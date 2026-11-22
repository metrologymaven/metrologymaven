//! CI surface-completeness and staleness check (spec node `AUTO.SURF.CHECK`).
//!
//! Reads the registry manifest, projects it with [`fid_surface::project`], parses each checked-in
//! surface's `surface-index.json` (`cli`, `mcp`, `py`, `docs`), reports completeness, orphan and
//! unexpected-MCP findings, then regenerates registered surfaces into a temporary directory and
//! compares every regenerated file byte-for-byte against `<generated>`.
//!
//! All collections are ordered (`BTreeMap` / `BTreeSet`); findings are sorted by `(kind, path,
//! surface, command)` and serialised as one canonical JSON document (REQ-AUTO.SURF.CHECK-005).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_surface::{project, McpProjection, ProjError, SurfaceProjection};

use crate::{generate, read_manifest, registered, SurfaceEmitter, INDEX_FILE};

/// The four surfaces checked for command-major completeness (REQ-AUTO.SURF.CHECK-001).
pub const CHECKED_SURFACES: [&str; 4] = ["cli", "mcp", "py", "docs"];

/// Maximum size in bytes of a single `surface-index.json` file read by the checker (bounded
/// parser, CONSTITUTION §8).
pub const MAX_INDEX_BYTES: usize = 32 * 1024 * 1024;

/// Maximum size in bytes of any generated file read during staleness comparison.
pub const MAX_GENERATED_FILE_BYTES: usize = 64 * 1024 * 1024;

/// Usage line of `cargo xtask surface-check`.
pub const CHECK_USAGE: &str = "usage: cargo xtask surface-check --manifest <path> --generated <dir>";

/// Category of a surface-check finding (REQ-AUTO.SURF.CHECK-001..004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingKind {
    MissingSurface,
    OrphanEntry,
    UnexpectedMcpEntry,
    StaleFile,
}

impl FindingKind {
    /// Stable wire name written in the JSON findings report.
    pub fn as_str(self) -> &'static str {
        match self {
            FindingKind::MissingSurface => "missing_surface",
            FindingKind::OrphanEntry => "orphan_entry",
            FindingKind::UnexpectedMcpEntry => "unexpected_mcp_entry",
            FindingKind::StaleFile => "stale_file",
        }
    }
}

/// Command major named by an index finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandRef {
    pub command_id: String,
    pub major: u32,
}

impl CommandRef {
    pub fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("command_id".to_string(), CValue::Str(self.command_id.clone()));
        o.insert("major".to_string(), CValue::Num(f64::from(self.major)));
        CValue::Obj(o)
    }
}

/// One completeness, orphan, unexpected-MCP or staleness finding (`AUTO.SURF.CHECK` Design).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: FindingKind,
    pub surface: String,
    pub command: Option<CommandRef>,
    pub path: Option<String>,
}

impl Finding {
    /// Sort key: finding kind wire name, then path, then surface, then command (REQ-005).
    fn sort_key(&self) -> (&'static str, &str, &str, &str, u32) {
        (
            self.kind.as_str(),
            self.path.as_deref().unwrap_or(""),
            self.surface.as_str(),
            self.command.as_ref().map(|c| c.command_id.as_str()).unwrap_or(""),
            self.command.as_ref().map(|c| c.major).unwrap_or(0),
        )
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("kind".to_string(), CValue::Str(self.kind.as_str().to_string()));
        o.insert("surface".to_string(), CValue::Str(self.surface.clone()));
        o.insert(
            "command".to_string(),
            match &self.command {
                Some(c) => c.to_cvalue(),
                None => CValue::Null,
            },
        );
        o.insert(
            "path".to_string(),
            match &self.path {
                Some(p) => CValue::Str(p.clone()),
                None => CValue::Null,
            },
        );
        CValue::Obj(o)
    }
}

/// Operational errors distinct from findings (`AUTO.SURF.CHECK` Design).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    ManifestParse { detail: String },
    IndexParse { surface: String, detail: String },
    IoError { path: String, detail: String },
}

impl CheckError {
    pub fn code(&self) -> &'static str {
        match self {
            CheckError::ManifestParse { .. } => "manifest_parse",
            CheckError::IndexParse { .. } => "index_parse",
            CheckError::IoError { .. } => "io_error",
        }
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::ManifestParse { detail } => write!(f, "manifest_parse: {detail}"),
            CheckError::IndexParse { surface, detail } => write!(f, "index_parse ({surface}): {detail}"),
            CheckError::IoError { path, detail } => write!(f, "io_error ({path}): {detail}"),
        }
    }
}

impl std::error::Error for CheckError {}

impl From<ProjError> for CheckError {
    fn from(e: ProjError) -> Self {
        CheckError::ManifestParse { detail: e.to_string() }
    }
}

/// Structured report written as canonical JSON to stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub findings: Vec<Finding>,
}

impl CheckReport {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("findings".to_string(), CValue::Arr(self.findings.iter().map(Finding::to_cvalue).collect()));
        CValue::Obj(o)
    }

    pub fn to_canonical_json(&self) -> Result<String, CheckError> {
        let bytes = to_canonical_bytes(&self.to_cvalue())
            .map_err(|e| CheckError::IoError { path: "<stdout>".to_string(), detail: e.to_string() })?;
        String::from_utf8(bytes)
            .map_err(|e| CheckError::IoError { path: "<stdout>".to_string(), detail: e.to_string() })
    }
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, CheckError> {
    let f = fs::File::open(path)
        .map_err(|e| CheckError::IoError { path: path.display().to_string(), detail: e.to_string() })?;
    let mut bytes = Vec::new();
    let max = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    f.take(max)
        .read_to_end(&mut bytes)
        .map_err(|e| CheckError::IoError { path: path.display().to_string(), detail: e.to_string() })?;
    if bytes.len() > limit {
        return Err(CheckError::IoError {
            path: path.display().to_string(),
            detail: format!("file exceeds bounded limit of {limit} bytes"),
        });
    }
    Ok(bytes)
}

/// Parses `<generated>/<surface>/surface-index.json` into the set of `(command_id, major)`.
fn read_surface_index(generated: &Path, surface: &str) -> Result<BTreeSet<CommandRef>, CheckError> {
    let path = generated.join(surface).join(INDEX_FILE);
    let bytes = read_bounded(&path, MAX_INDEX_BYTES)?;
    let val = parse(&bytes)
        .map_err(|e| CheckError::IndexParse { surface: surface.to_string(), detail: format!("json: {e}") })?;
    let CValue::Obj(root) = val else {
        return Err(CheckError::IndexParse {
            surface: surface.to_string(),
            detail: "surface-index.json root must be a JSON object".to_string(),
        });
    };
    if let Some(CValue::Str(declared)) = root.get("surface") {
        if declared != surface {
            return Err(CheckError::IndexParse {
                surface: surface.to_string(),
                detail: format!("surface-index.json declares surface {declared:?}, expected {surface:?}"),
            });
        }
    }
    let Some(CValue::Arr(cmds)) = root.get("commands") else {
        return Err(CheckError::IndexParse {
            surface: surface.to_string(),
            detail: "surface-index.json missing `commands` array".to_string(),
        });
    };
    let mut set = BTreeSet::new();
    for (idx, item) in cmds.iter().enumerate() {
        let CValue::Obj(o) = item else {
            return Err(CheckError::IndexParse {
                surface: surface.to_string(),
                detail: format!("commands[{idx}] must be an object"),
            });
        };
        let Some(CValue::Str(command_id)) = o.get("command_id") else {
            return Err(CheckError::IndexParse {
                surface: surface.to_string(),
                detail: format!("commands[{idx}] missing string `command_id`"),
            });
        };
        let major = match o.get("major") {
            Some(CValue::Num(n)) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX) => {
                *n as u32
            }
            Some(CValue::U64(n)) => u32::try_from(*n).map_err(|_| CheckError::IndexParse {
                surface: surface.to_string(),
                detail: format!("commands[{idx}] major out of range"),
            })?,
            _ => {
                return Err(CheckError::IndexParse {
                    surface: surface.to_string(),
                    detail: format!("commands[{idx}] missing integer `major`"),
                });
            }
        };
        set.insert(CommandRef { command_id: command_id.clone(), major });
    }
    Ok(set)
}

/// True when a projected command is expected to have an MCP tool entry (REQ-001, REQ-003).
fn mcp_expected(c: &SurfaceProjection) -> bool {
    c.mcp_projection != McpProjection::None && c.exposure.iter().any(|s| s == "mcp")
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_temp_dir() -> Result<PathBuf, CheckError> {
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fid-surface-check-{}-{n}", std::process::id()));
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir)
        .map_err(|e| CheckError::IoError { path: dir.display().to_string(), detail: e.to_string() })?;
    Ok(dir)
}

/// Collects all files under `root` as `/`-separated relative paths in sorted order.
fn collect_tree(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, CheckError> {
    let mut out = BTreeMap::new();
    if !root.exists() {
        return Ok(out);
    }
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let mut entries: Vec<(String, PathBuf)> = Vec::new();
        for e in fs::read_dir(&dir)
            .map_err(|e| CheckError::IoError { path: dir.display().to_string(), detail: e.to_string() })?
        {
            let e = e.map_err(|e| CheckError::IoError { path: dir.display().to_string(), detail: e.to_string() })?;
            let name = e.file_name().to_string_lossy().into_owned();
            entries.push((name, e.path()));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path) in entries {
            let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| CheckError::IoError { path: path.display().to_string(), detail: e.to_string() })?;
            if meta.is_dir() && !meta.file_type().is_symlink() {
                stack.push((path, rel));
            } else {
                let bytes = read_bounded(&path, MAX_GENERATED_FILE_BYTES)?;
                out.insert(rel, bytes);
            }
        }
    }
    Ok(out)
}

/// Runs surface completeness and staleness checks against `generated` using the registered
/// emitters ([`registered`]).
pub fn check_surfaces(manifest_bytes: &[u8], generated: &Path) -> Result<CheckReport, CheckError> {
    let emitters = registered();
    let refs: Vec<&dyn SurfaceEmitter> = emitters.iter().map(|e| e.as_ref()).collect();
    check_surfaces_with_emitters(manifest_bytes, generated, &refs)
}

/// Runs surface completeness checks across [`CHECKED_SURFACES`] (`cli`, `mcp`, `py`, `docs`) and
/// regenerates `emitters` into a temporary directory to detect any byte-level staleness
/// (REQ-AUTO.SURF.CHECK-001..005).
pub fn check_surfaces_with_emitters(
    manifest_bytes: &[u8],
    generated: &Path,
    emitters: &[&dyn SurfaceEmitter],
) -> Result<CheckReport, CheckError> {
    let proj = project(manifest_bytes)?;
    let mut findings: Vec<Finding> = Vec::new();

    let all_manifest_cmds: BTreeSet<CommandRef> = proj
        .commands
        .iter()
        .map(|c| CommandRef { command_id: c.command_id.as_str().to_string(), major: c.major })
        .collect();
    let expected_mcp_cmds: BTreeSet<CommandRef> = proj
        .commands
        .iter()
        .filter(|c| mcp_expected(c))
        .map(|c| CommandRef { command_id: c.command_id.as_str().to_string(), major: c.major })
        .collect();
    let forbidden_mcp_cmds: BTreeSet<CommandRef> = proj
        .commands
        .iter()
        .filter(|c| !mcp_expected(c))
        .map(|c| CommandRef { command_id: c.command_id.as_str().to_string(), major: c.major })
        .collect();

    // 1. Read every checked surface's `surface-index.json` and check completeness, orphans and
    //    unexpected MCP entries (REQ-001, REQ-002, REQ-003).
    for surface in CHECKED_SURFACES {
        let indexed = read_surface_index(generated, surface)?;
        let expected = if surface == "mcp" { &expected_mcp_cmds } else { &all_manifest_cmds };

        for cmd in expected.difference(&indexed) {
            findings.push(Finding {
                kind: FindingKind::MissingSurface,
                surface: surface.to_string(),
                command: Some(cmd.clone()),
                path: Some(format!("{surface}/{INDEX_FILE}")),
            });
        }

        for cmd in indexed.difference(&all_manifest_cmds) {
            findings.push(Finding {
                kind: FindingKind::OrphanEntry,
                surface: surface.to_string(),
                command: Some(cmd.clone()),
                path: Some(format!("{surface}/{INDEX_FILE}")),
            });
        }

        if surface == "mcp" {
            for cmd in indexed.intersection(&forbidden_mcp_cmds) {
                findings.push(Finding {
                    kind: FindingKind::UnexpectedMcpEntry,
                    surface: surface.to_string(),
                    command: Some(cmd.clone()),
                    path: Some(format!("{surface}/{INDEX_FILE}")),
                });
            }
        }
    }

    // 2. Regenerate every registered surface into a temporary directory and compare trees
    //    byte-for-byte (REQ-004).
    let tmp = fresh_temp_dir()?;
    let regen_res = generate(manifest_bytes, emitters, &tmp);
    let compare_res = match regen_res {
        Ok(outputs) => {
            let mut cmp_err = None;
            for out in &outputs {
                let surface = out.surface.as_str();
                let fresh = match collect_tree(&tmp.join(surface)) {
                    Ok(m) => m,
                    Err(e) => {
                        cmp_err = Some(e);
                        break;
                    }
                };
                let actual = match collect_tree(&generated.join(surface)) {
                    Ok(m) => m,
                    Err(e) => {
                        cmp_err = Some(e);
                        break;
                    }
                };
                let all_paths: BTreeSet<&String> = fresh.keys().chain(actual.keys()).collect();
                for rel in all_paths {
                    if fresh.get(rel) != actual.get(rel) {
                        findings.push(Finding {
                            kind: FindingKind::StaleFile,
                            surface: surface.to_string(),
                            command: None,
                            path: Some(format!("{surface}/{rel}")),
                        });
                    }
                }
            }
            match cmp_err {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
        Err(e) => Err(CheckError::ManifestParse { detail: e.to_string() }),
    };
    let _ = fs::remove_dir_all(&tmp);
    compare_res?;

    findings.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    Ok(CheckReport { findings })
}

/// CLI entry point for `cargo xtask surface-check --manifest <path> --generated <dir>`.
///
/// Exit codes: `0` clean (empty findings JSON); `1` findings present (JSON printed to stdout);
/// `2` usage or operational error (`manifest_parse`, `index_parse`, `io_error` on stderr).
pub fn check_cli(args: &[String]) -> ExitCode {
    let (mut manifest, mut generated) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--manifest", Some(v)) => manifest = Some(PathBuf::from(v)),
            ("--generated", Some(v)) => generated = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{CHECK_USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(manifest), Some(generated)) = (manifest, generated) else {
        eprintln!("surface-check: --manifest and --generated are required\n{CHECK_USAGE}");
        return ExitCode::from(2);
    };
    let bytes = match read_manifest(&manifest) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("surface-check: io_error ({}): {e}", manifest.display());
            return ExitCode::from(2);
        }
    };
    match check_surfaces(&bytes, &generated) {
        Ok(report) => match report.to_canonical_json() {
            Ok(json) => {
                println!("{json}");
                if report.is_clean() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Err(e) => {
                eprintln!("surface-check: {e}");
                ExitCode::from(2)
            }
        },
        Err(e) => {
            eprintln!("surface-check: {e}");
            ExitCode::from(2)
        }
    }
}
