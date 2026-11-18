//! Workflow catalog per MCP server profile and its CI check (spec node `AGENT.SKILLS.CATALOG`).
//!
//! Inputs:
//! - the registry manifest (`CORE.REG.MAN` format) with its `task_guides` array; the manifest
//!   digest is the SHA-256 of the exact manifest bytes (REQ-CORE.REG.MAN-002);
//! - the profile index written by `AUTO.MCP.SRV` (`mcp-profiles.json`); only `profiles[].name`
//!   is read;
//! - the generated Skills folder (`AUTO.MCP.SKILLS`): `<skills>/<guide-id-with-hyphens>/SKILL.md`,
//!   whose front matter names `manifest_digest`;
//! - the catalog file `agent/catalog.json` and the recipe files `<recipes>/<recipe-id>.json`.
//!
//! The check reports every finding it can (typed codes), and only when there is none returns the
//! [`CatalogIndex`]: workflow ID to catalog version, plus the manifest digest, sorted by workflow ID.

use fid_canon::CValue;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// `format` tag of the catalog file.
pub const CATALOG_FORMAT: &str = "fid.agent-catalog";
/// The one catalog file version this crate reads.
pub const CATALOG_VERSION: u32 = 1;
/// The profile authored by `AUTO.MCP.SRV`; every other profile is an app profile.
pub const CORE_PROFILE: &str = "core";
/// Fewest workflows an app profile may list (REQ-AGENT.SKILLS.CATALOG-002) [unverified that 5 is enough].
pub const MIN_APP_WORKFLOWS: usize = 5;
/// Largest catalog, profile index, `SKILL.md` or recipe file read, in bytes.
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
/// Deepest JSON nesting accepted in any input.
const MAX_DEPTH: usize = 64;

/// Typed finding codes with stable wire strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    /// The workflow names a task guide absent from the manifest.
    MissingGuide,
    /// The guide's profile differs from the workflow's profile.
    ProfileMismatch,
    /// The generated Skill names another (or no) manifest digest.
    StaleSkill,
    /// No generated Skill exists for the guide.
    MissingSkill,
    /// An app profile lists fewer than [`MIN_APP_WORKFLOWS`] workflows.
    TooFewWorkflows,
    /// Two catalog rows share a workflow ID.
    DuplicateWorkflow,
    /// A catalog profile is absent from the profile index.
    UnknownProfile,
    /// A recipe file named by a workflow does not exist.
    MissingRecipe,
    /// The catalog file is malformed.
    BadCatalog,
    /// The manifest, the profile index or another input cannot be read or is malformed.
    BadInput,
}

impl Code {
    /// Stable machine-readable code.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::MissingGuide => "missing_guide",
            Code::ProfileMismatch => "profile_mismatch",
            Code::StaleSkill => "stale_skill",
            Code::MissingSkill => "missing_skill",
            Code::TooFewWorkflows => "too_few_workflows",
            Code::DuplicateWorkflow => "duplicate_workflow",
            Code::UnknownProfile => "unknown_profile",
            Code::MissingRecipe => "missing_recipe",
            Code::BadCatalog => "bad_catalog",
            Code::BadInput => "bad_input",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One check failure: a code, the workflow ID or profile it names, and a human detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub code: Code,
    pub subject: String,
    pub detail: String,
}

impl Finding {
    fn new(code: Code, subject: &str, detail: impl Into<String>) -> Finding {
        Finding { code, subject: subject.to_string(), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.code, self.subject, self.detail)
    }
}

/// One curated workflow of a profile (REQ-AGENT.SKILLS.CATALOG-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRow {
    pub profile: String,
    pub workflow_id: String,
    pub title: String,
    pub guide_id: String,
    pub recipes: Vec<String>,
}

impl CatalogRow {
    /// The row as hashed into its catalog version: all five fields, `recipes` in file order.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("profile".to_string(), CValue::Str(self.profile.clone()));
        m.insert("workflow_id".to_string(), CValue::Str(self.workflow_id.clone()));
        m.insert("title".to_string(), CValue::Str(self.title.clone()));
        m.insert("guide_id".to_string(), CValue::Str(self.guide_id.clone()));
        m.insert("recipes".to_string(), CValue::Arr(self.recipes.iter().cloned().map(CValue::Str).collect()));
        CValue::Obj(m)
    }
}

/// One line of the version index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    pub workflow_id: String,
    pub catalog_version: Digest,
}

/// The version index `catalog-index.json` (REQ-AGENT.SKILLS.CATALOG-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogIndex {
    pub manifest_digest: Digest,
    /// Sorted by workflow ID.
    pub rows: Vec<IndexRow>,
}

impl CatalogIndex {
    /// `{ "manifest_digest", "rows": [{ "catalog_version", "workflow_id" }] }`.
    pub fn to_cvalue(&self) -> CValue {
        let rows = self
            .rows
            .iter()
            .map(|r| {
                let mut m = BTreeMap::new();
                m.insert("workflow_id".to_string(), CValue::Str(r.workflow_id.clone()));
                m.insert("catalog_version".to_string(), CValue::Str(r.catalog_version.as_str().to_string()));
                CValue::Obj(m)
            })
            .collect();
        let mut m = BTreeMap::new();
        m.insert("manifest_digest".to_string(), CValue::Str(self.manifest_digest.as_str().to_string()));
        m.insert("rows".to_string(), CValue::Arr(rows));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical bytes of [`CatalogIndex::to_cvalue`] (no trailing newline).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, Finding> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
            .map_err(|e| Finding::new(Code::BadInput, "catalog-index", format!("cannot serialise: {e}")))
    }
}

/// Paths of every input of the check.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub manifest: PathBuf,
    pub profiles: PathBuf,
    pub catalog: PathBuf,
    pub skills: PathBuf,
    pub recipes: PathBuf,
}

/// Skill folder name of a guide: the guide ID with dots and underscores turned to hyphens
/// (`AUTO.MCP.SKILLS` design).
pub fn skill_dir_name(guide_id: &str) -> String {
    guide_id.chars().map(|c| if c == '.' || c == '_' { '-' } else { c }).collect()
}

/// `^[a-z][a-z0-9]*\.[a-z][a-z0-9_]*$`, the task-guide ID pattern (REQ-CORE.REG.DEF.GUIDE-001),
/// also used for workflow and recipe IDs so they are safe as file names.
fn is_id(s: &str) -> bool {
    let Some((a, b)) = s.split_once('.') else { return false };
    let head = |p: &str| p.as_bytes().first().is_some_and(u8::is_ascii_lowercase);
    head(a)
        && head(b)
        && a.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// Catalog version of one workflow: SHA-256 of the RFC 8785 form of
/// `{ "guide": <guide declaration>, "recipes": [{ "id", "sha256" }], "row": <row> }`, where each
/// recipe's `sha256` is the digest of that recipe file's exact bytes (REQ-AGENT.SKILLS.CATALOG-004).
pub fn catalog_version(row: &CatalogRow, guide: &CValue, recipes: &[(String, Digest)]) -> Result<Digest, Finding> {
    let recipes = recipes
        .iter()
        .map(|(id, d)| {
            let mut m = BTreeMap::new();
            m.insert("id".to_string(), CValue::Str(id.clone()));
            m.insert("sha256".to_string(), CValue::Str(d.as_str().to_string()));
            CValue::Obj(m)
        })
        .collect();
    let mut m = BTreeMap::new();
    m.insert("guide".to_string(), guide.clone());
    m.insert("recipes".to_string(), CValue::Arr(recipes));
    m.insert("row".to_string(), row.to_cvalue());
    fid_canon::digest(&CValue::Obj(m))
        .map(|d| Digest::from_bytes(d.0))
        .map_err(|e| Finding::new(Code::BadInput, &row.workflow_id, format!("cannot hash: {e}")))
}

// ------------------------------------------------------------------ parsing

fn bad_catalog(detail: impl Into<String>) -> Finding {
    Finding::new(Code::BadCatalog, "catalog", detail)
}

fn str_field(m: &BTreeMap<String, CValue>, key: &str, at: &str) -> Result<String, Finding> {
    match m.get(key) {
        Some(CValue::Str(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(bad_catalog(format!("{at}: `{key}` must be a non-empty string"))),
    }
}

fn parse_row(v: &CValue, i: usize) -> Result<CatalogRow, Finding> {
    let at = format!("workflows[{i}]");
    let CValue::Obj(m) = v else { return Err(bad_catalog(format!("{at}: not an object"))) };
    for k in m.keys() {
        if !matches!(k.as_str(), "profile" | "workflow_id" | "title" | "guide_id" | "recipes") {
            return Err(bad_catalog(format!("{at}: unknown field `{k}`")));
        }
    }
    let row_recipes = match m.get("recipes") {
        None => Vec::new(),
        Some(CValue::Arr(a)) => {
            let mut out: Vec<String> = Vec::new();
            for r in a {
                match r {
                    CValue::Str(s) if is_id(s) && !out.contains(s) => out.push(s.clone()),
                    _ => return Err(bad_catalog(format!("{at}: `recipes` must hold distinct recipe IDs"))),
                }
            }
            out
        }
        Some(_) => return Err(bad_catalog(format!("{at}: `recipes` must be an array"))),
    };
    let row = CatalogRow {
        profile: str_field(m, "profile", &at)?,
        workflow_id: str_field(m, "workflow_id", &at)?,
        title: str_field(m, "title", &at)?,
        guide_id: str_field(m, "guide_id", &at)?,
        recipes: row_recipes,
    };
    if !is_id(&row.workflow_id) {
        return Err(bad_catalog(format!("{at}: bad workflow ID {:?}", row.workflow_id)));
    }
    if !is_id(&row.guide_id) {
        return Err(bad_catalog(format!("{at}: bad guide ID {:?}", row.guide_id)));
    }
    Ok(row)
}

/// Parses a catalog file: `{ "format": "fid.agent-catalog", "version": 1, "workflows": [rows] }`.
pub fn parse_catalog(bytes: &[u8]) -> Result<Vec<CatalogRow>, Finding> {
    let v = fid_canon::parse_with_limits(bytes, MAX_FILE_BYTES, MAX_DEPTH)
        .map_err(|e| bad_catalog(format!("not valid JSON: {e}")))?;
    let CValue::Obj(top) = v else { return Err(bad_catalog("not a JSON object")) };
    for k in top.keys() {
        if !matches!(k.as_str(), "format" | "version" | "workflows") {
            return Err(bad_catalog(format!("unknown field `{k}`")));
        }
    }
    match top.get("format") {
        Some(CValue::Str(s)) if s == CATALOG_FORMAT => {}
        _ => return Err(bad_catalog(format!("`format` must be {CATALOG_FORMAT:?}"))),
    }
    match top.get("version") {
        Some(CValue::Num(n)) if n.to_bits() == f64::from(CATALOG_VERSION).to_bits() => {}
        _ => return Err(bad_catalog(format!("`version` must be {CATALOG_VERSION}"))),
    }
    let Some(CValue::Arr(rows)) = top.get("workflows") else {
        return Err(bad_catalog("`workflows` must be an array"));
    };
    rows.iter().enumerate().map(|(i, r)| parse_row(r, i)).collect()
}

enum ReadErr {
    NotFound,
    Other(String),
}

fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>, ReadErr> {
    let file = std::fs::File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ReadErr::NotFound,
        _ => ReadErr::Other(format!("{}: {e}", path.display())),
    })?;
    let mut buf = Vec::new();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    file.take(limit).read_to_end(&mut buf).map_err(|e| ReadErr::Other(format!("{}: {e}", path.display())))?;
    if buf.len() > max {
        return Err(ReadErr::Other(format!("{}: larger than {max} bytes", path.display())));
    }
    Ok(buf)
}

fn read_input(path: &Path, max: usize, what: &str) -> Result<Vec<u8>, Finding> {
    read_bounded(path, max).map_err(|e| match e {
        ReadErr::NotFound => Finding::new(Code::BadInput, what, format!("{}: not found", path.display())),
        ReadErr::Other(s) => Finding::new(Code::BadInput, what, s),
    })
}

fn parse_json(bytes: &[u8], max: usize, what: &str) -> Result<CValue, Finding> {
    fid_canon::parse_with_limits(bytes, max, MAX_DEPTH)
        .map_err(|e| Finding::new(Code::BadInput, what, format!("not valid JSON: {e}")))
}

/// Guide ID to (profile, full declaration), from the manifest's `task_guides` array.
fn manifest_guides(manifest: &CValue) -> Result<BTreeMap<String, (String, CValue)>, Finding> {
    let bad = |d: String| Finding::new(Code::BadInput, "manifest", d);
    let CValue::Obj(top) = manifest else { return Err(bad("not a JSON object".into())) };
    let guides = match top.get("task_guides") {
        None => return Ok(BTreeMap::new()),
        Some(CValue::Arr(a)) => a,
        Some(_) => return Err(bad("`task_guides` must be an array".into())),
    };
    let mut out = BTreeMap::new();
    for (i, g) in guides.iter().enumerate() {
        let CValue::Obj(m) = g else { return Err(bad(format!("task_guides[{i}]: not an object"))) };
        let (Some(CValue::Str(id)), Some(CValue::Str(profile))) = (m.get("id"), m.get("profile")) else {
            return Err(bad(format!("task_guides[{i}]: `id` and `profile` must be strings")));
        };
        if out.insert(id.clone(), (profile.clone(), g.clone())).is_some() {
            return Err(bad(format!("task_guides: guide {id:?} declared twice")));
        }
    }
    Ok(out)
}

/// Profile names from the `AUTO.MCP.SRV` profile index (`profiles[].name`).
fn profile_names(index: &CValue) -> Result<BTreeSet<String>, Finding> {
    let bad = |d: String| Finding::new(Code::BadInput, "profiles", d);
    let CValue::Obj(top) = index else { return Err(bad("not a JSON object".into())) };
    let Some(CValue::Arr(profiles)) = top.get("profiles") else {
        return Err(bad("`profiles` must be an array".into()));
    };
    let mut out = BTreeSet::new();
    for (i, p) in profiles.iter().enumerate() {
        let name = match p {
            CValue::Obj(m) => match m.get("name") {
                Some(CValue::Str(s)) => s.clone(),
                _ => return Err(bad(format!("profiles[{i}]: `name` must be a string"))),
            },
            _ => return Err(bad(format!("profiles[{i}]: not an object"))),
        };
        if !out.insert(name.clone()) {
            return Err(bad(format!("profile {name:?} listed twice")));
        }
    }
    Ok(out)
}

/// The `manifest_digest` value in a `SKILL.md` front matter (`---` fenced, `key: value` lines).
fn skill_manifest_digest(text: &str) -> Option<&str> {
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            return None;
        }
        if let Some(v) = line.strip_prefix("manifest_digest:") {
            return Some(v.trim());
        }
    }
    None
}

// ------------------------------------------------------------------ check

/// Runs the catalog check. Returns the version index when there is no finding, and otherwise
/// every finding in a deterministic order: duplicate workflow IDs, unknown profiles, per-row
/// findings in catalog order, then app profiles with too few workflows by profile name.
pub fn check(inputs: &Inputs) -> Result<CatalogIndex, Vec<Finding>> {
    let manifest_bytes = read_input(&inputs.manifest, fid_canon::MAX_INPUT_BYTES, "manifest").map_err(one)?;
    let manifest_digest = Digest::from_bytes(fid_canon::digest_bytes(&manifest_bytes).0);
    let manifest = parse_json(&manifest_bytes, fid_canon::MAX_INPUT_BYTES, "manifest").map_err(one)?;
    let guides = manifest_guides(&manifest).map_err(one)?;
    let profiles_bytes = read_input(&inputs.profiles, MAX_FILE_BYTES, "profiles").map_err(one)?;
    let profiles =
        profile_names(&parse_json(&profiles_bytes, MAX_FILE_BYTES, "profiles").map_err(one)?).map_err(one)?;
    let catalog_bytes = read_input(&inputs.catalog, MAX_FILE_BYTES, "catalog").map_err(one)?;
    let rows = parse_catalog(&catalog_bytes).map_err(one)?;

    let mut findings = Vec::new();

    // REQ-005: repeated workflow IDs and unknown profiles.
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &rows {
        *counts.entry(r.workflow_id.as_str()).or_insert(0) += 1;
    }
    for (id, n) in &counts {
        if *n > 1 {
            findings.push(Finding::new(
                Code::DuplicateWorkflow,
                id,
                format!("{n} catalog rows share this workflow ID"),
            ));
        }
    }
    let catalog_profiles: BTreeSet<&str> = rows.iter().map(|r| r.profile.as_str()).collect();
    for p in catalog_profiles {
        if !profiles.contains(p) {
            findings.push(Finding::new(Code::UnknownProfile, p, "profile is not in the profile index"));
        }
    }

    // REQ-003 and REQ-004: per-row checks, then the catalog version.
    let mut index_rows = Vec::new();
    for row in &rows {
        let before = findings.len();
        let wf = row.workflow_id.as_str();
        let Some((guide_profile, guide)) = guides.get(&row.guide_id) else {
            findings.push(Finding::new(
                Code::MissingGuide,
                wf,
                format!("guide {:?} is not in the manifest", row.guide_id),
            ));
            continue;
        };
        if *guide_profile != row.profile {
            findings.push(Finding::new(
                Code::ProfileMismatch,
                wf,
                format!(
                    "guide {:?} belongs to profile {guide_profile:?}, the row names {:?}",
                    row.guide_id, row.profile
                ),
            ));
        }
        let skill = inputs.skills.join(skill_dir_name(&row.guide_id)).join("SKILL.md");
        match read_bounded(&skill, MAX_FILE_BYTES) {
            Err(ReadErr::NotFound) => findings.push(Finding::new(
                Code::MissingSkill,
                wf,
                format!("no generated Skill for guide {:?} at {}", row.guide_id, skill.display()),
            )),
            Err(ReadErr::Other(e)) => findings.push(Finding::new(Code::BadInput, wf, e)),
            Ok(bytes) => {
                let named = std::str::from_utf8(&bytes).ok().and_then(skill_manifest_digest);
                if named != Some(manifest_digest.as_str()) {
                    findings.push(Finding::new(
                        Code::StaleSkill,
                        wf,
                        format!(
                            "Skill for guide {:?} names manifest digest {}, current is {}",
                            row.guide_id,
                            named.unwrap_or("(none)"),
                            manifest_digest
                        ),
                    ));
                }
            }
        }
        let mut recipes = Vec::new();
        for id in &row.recipes {
            let path = inputs.recipes.join(format!("{id}.json"));
            match read_bounded(&path, MAX_FILE_BYTES) {
                Ok(bytes) => recipes.push((id.clone(), Digest::from_bytes(fid_canon::digest_bytes(&bytes).0))),
                Err(ReadErr::NotFound) => findings.push(Finding::new(
                    Code::MissingRecipe,
                    wf,
                    format!("recipe file {} not found", path.display()),
                )),
                Err(ReadErr::Other(e)) => findings.push(Finding::new(Code::BadInput, wf, e)),
            }
        }
        if findings.len() == before {
            match catalog_version(row, guide, &recipes) {
                Ok(v) => index_rows.push(IndexRow { workflow_id: row.workflow_id.clone(), catalog_version: v }),
                Err(f) => findings.push(f),
            }
        }
    }

    // REQ-002: every app profile lists enough workflows.
    for p in profiles.iter().filter(|p| p.as_str() != CORE_PROFILE) {
        let n = rows.iter().filter(|r| r.profile == *p).count();
        if n < MIN_APP_WORKFLOWS {
            findings.push(Finding::new(
                Code::TooFewWorkflows,
                p,
                format!("app profile lists {n} workflows, at least {MIN_APP_WORKFLOWS} are required"),
            ));
        }
    }

    if !findings.is_empty() {
        return Err(findings);
    }
    index_rows.sort_by(|a, b| a.workflow_id.cmp(&b.workflow_id));
    Ok(CatalogIndex { manifest_digest, rows: index_rows })
}

fn one(f: Finding) -> Vec<Finding> {
    vec![f]
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert!(is_id("demo.align_scan"));
        assert!(is_id("core2.x"));
        assert!(!is_id("demo"));
        assert!(!is_id("Demo.x"));
        assert!(!is_id("demo.x.y"));
        assert!(!is_id("demo._x"));
        assert!(!is_id("../etc.x"));
    }

    #[test]
    fn skill_dir() {
        assert_eq!(skill_dir_name("demo.align_scan"), "demo-align-scan");
    }

    #[test]
    fn front_matter() {
        assert_eq!(skill_manifest_digest("---\nname: a\nmanifest_digest: sha256:ab\n---\nbody"), Some("sha256:ab"));
        assert_eq!(skill_manifest_digest("---\r\nmanifest_digest:  x \r\n---\r\n"), Some("x"));
        assert_eq!(skill_manifest_digest("---\nname: a\n---\nmanifest_digest: x\n"), None);
        assert_eq!(skill_manifest_digest("name: a\n"), None);
    }

    #[test]
    fn catalog_shape() {
        let ok = br#"{"format":"fid.agent-catalog","version":1,"workflows":[
            {"profile":"core","workflow_id":"core.a","title":"A","guide_id":"core.a"}]}"#;
        let rows = parse_catalog(ok).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].recipes.is_empty());
        let bad = [
            &br#"{"format":"x","version":1,"workflows":[]}"#[..],
            br#"{"format":"fid.agent-catalog","version":2,"workflows":[]}"#,
            br#"{"format":"fid.agent-catalog","version":1,"workflows":[],"extra":1}"#,
            br#"{"format":"fid.agent-catalog","version":1,"workflows":[{"profile":"core","workflow_id":"core.a","title":"A","guide_id":"core.a","x":1}]}"#,
            br#"{"format":"fid.agent-catalog","version":1,"workflows":[{"profile":"core","workflow_id":"core.a","title":"","guide_id":"core.a"}]}"#,
            br#"{"format":"fid.agent-catalog","version":1,"workflows":[{"profile":"core","workflow_id":"core.a","title":"A","guide_id":"../x"}]}"#,
            br#"{"format":"fid.agent-catalog","version":1,"workflows":[{"profile":"core","workflow_id":"core.a","title":"A","guide_id":"core.a","recipes":["core.r","core.r"]}]}"#,
        ];
        for b in bad {
            assert_eq!(parse_catalog(b).unwrap_err().code, Code::BadCatalog);
        }
    }
}
