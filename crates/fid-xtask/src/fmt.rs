//! Cross-platform batched workspace formatter (`cargo xtask fmt`).
//!
//! Invoking `cargo fmt --all --check` passes every `.rs` path in the workspace to a single
//! `rustfmt` child process, exceeding Windows `CreateProcessW`'s 32,767-character command-line
//! limit (`os error 206`) once a workspace grows past a few hundred source files. This module
//! discovers workspace members once via `cargo metadata --no-deps --format-version 1`, walks
//! each member's `.rs` files, greedily batches packages within a configurable byte budget, and
//! falls back to chunked `rustfmt` invocations if any single crate exceeds the budget.

use fid_canon::{parse, CValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Conservative default per-invocation argument-length budget (well below Windows' 32 KiB limit).
pub const DEFAULT_MAX_BATCH_BYTES: usize = 12_000;

/// CLI usage line for `cargo xtask fmt`.
pub const USAGE: &str = "usage: cargo xtask fmt [--check] [--workspace <dir>] [--max-batch-bytes <N>]";

/// Formatting metadata for a single workspace member package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFmtInfo {
    pub name: String,
    pub edition: String,
    pub files: Vec<PathBuf>,
    pub estimated_bytes: usize,
}

/// One planned formatting invocation guaranteed to stay within the batch byte budget
/// (unless a single file path itself exceeds `max_batch_bytes`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FmtBatch {
    CargoFmt { packages: Vec<String>, estimated_bytes: usize },
    DirectRustfmt { package: String, edition: String, files: Vec<PathBuf>, estimated_bytes: usize },
}

/// Runtime options for [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub workspace: PathBuf,
    pub check: bool,
    pub max_batch_bytes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self { workspace: PathBuf::from("."), check: false, max_batch_bytes: DEFAULT_MAX_BATCH_BYTES }
    }
}

/// Summary of a completed `fmt` run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub passed: bool,
    pub packages: usize,
    pub batches: usize,
}

/// Estimates the command-line byte footprint of passing `path` (plus quoting/spacing) to `rustfmt`.
pub fn estimate_file_arg_bytes(path: &Path) -> usize {
    path.to_string_lossy().len().saturating_add(3)
}

/// Recursively collects `.rs` files under `dir`, skipping `target/`, hidden directories (`.git/`,
/// etc.), and nested sub-packages (`Cargo.toml` in a subdirectory). Output is deterministically
/// sorted.
pub fn collect_rs_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_rs_files_inner(dir, dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_rs_files_inner(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read directory {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read entry in {}: {e}", dir.display()))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if file_type.is_dir() {
            if name_str == "target" || name_str.starts_with('.') {
                continue;
            }
            if path != root && path.join("Cargo.toml").is_file() {
                continue;
            }
            collect_rs_files_inner(root, &path, out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Queries `cargo metadata --no-deps --format-version 1` for `workspace` and collects `.rs`
/// file footprints for every workspace member.
pub fn load_workspace_packages(workspace: &Path) -> Result<Vec<PackageFmtInfo>, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest_path = workspace.join("Cargo.toml");
    let out = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1", "--manifest-path"])
        .arg(&manifest_path)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    parse_workspace_metadata(&out.stdout)
}

/// Parses `cargo metadata --no-deps --format-version 1` JSON bytes and collects member `.rs`
/// files on disk.
pub fn parse_workspace_metadata(json: &[u8]) -> Result<Vec<PackageFmtInfo>, String> {
    let root = parse(json).map_err(|e| format!("invalid cargo metadata JSON: {e}"))?;
    let CValue::Obj(root_obj) = root else {
        return Err("invalid cargo metadata JSON: expected root object".to_string());
    };
    let Some(CValue::Arr(packages_arr)) = root_obj.get("packages") else {
        return Err("invalid cargo metadata JSON: missing packages array".to_string());
    };
    let Some(CValue::Arr(members_arr)) = root_obj.get("workspace_members") else {
        return Err("invalid cargo metadata JSON: missing workspace_members array".to_string());
    };

    struct RawPkg {
        name: String,
        edition: String,
        manifest_path: PathBuf,
    }

    let mut by_id: BTreeMap<String, RawPkg> = BTreeMap::new();
    for pkg in packages_arr {
        let CValue::Obj(obj) = pkg else {
            return Err("invalid cargo metadata JSON: package entry is not an object".to_string());
        };
        let Some(CValue::Str(id)) = obj.get("id") else {
            return Err("invalid cargo metadata JSON: package missing id".to_string());
        };
        let Some(CValue::Str(name)) = obj.get("name") else {
            return Err("invalid cargo metadata JSON: package missing name".to_string());
        };
        let Some(CValue::Str(manifest_path)) = obj.get("manifest_path") else {
            return Err("invalid cargo metadata JSON: package missing manifest_path".to_string());
        };
        let edition = match obj.get("edition") {
            Some(CValue::Str(ed)) if !ed.is_empty() => ed.clone(),
            _ => "2021".to_string(),
        };
        by_id.insert(id.clone(), RawPkg { name: name.clone(), edition, manifest_path: PathBuf::from(manifest_path) });
    }

    let mut infos = Vec::with_capacity(members_arr.len());
    for member in members_arr {
        let CValue::Str(member_id) = member else {
            return Err("invalid cargo metadata JSON: workspace_members entry is not a string".to_string());
        };
        let pkg = by_id
            .get(member_id)
            .ok_or_else(|| format!("invalid cargo metadata JSON: unknown workspace member {member_id}"))?;
        let pkg_dir = pkg
            .manifest_path
            .parent()
            .ok_or_else(|| format!("manifest path has no parent directory: {}", pkg.manifest_path.display()))?;
        let files = collect_rs_files(pkg_dir)?;
        let estimated_bytes = files.iter().map(|p| estimate_file_arg_bytes(p)).fold(0usize, usize::saturating_add);
        infos.push(PackageFmtInfo { name: pkg.name.clone(), edition: pkg.edition.clone(), files, estimated_bytes });
    }

    infos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(infos)
}

/// Packs workspace packages into bounded formatting batches.
///
/// Packages whose estimated path footprint fits within `max_batch_bytes` are greedily packed into
/// [`FmtBatch::CargoFmt`] batches. Any single package exceeding `max_batch_bytes` is split into
/// [`FmtBatch::DirectRustfmt`] file chunks, each bounded by `max_batch_bytes` (with at least one
/// file per chunk to guarantee forward progress if a single path exceeds the limit).
pub fn plan_batches(packages: &[PackageFmtInfo], max_batch_bytes: usize) -> Vec<FmtBatch> {
    let limit = max_batch_bytes.max(1);
    let mut batches = Vec::new();
    let mut current_pkgs: Vec<String> = Vec::new();
    let mut current_bytes: usize = 0;

    for pkg in packages {
        if pkg.estimated_bytes > limit {
            if !current_pkgs.is_empty() {
                batches.push(FmtBatch::CargoFmt {
                    packages: std::mem::take(&mut current_pkgs),
                    estimated_bytes: current_bytes,
                });
                current_bytes = 0;
            }
            if pkg.files.is_empty() {
                batches.push(FmtBatch::CargoFmt {
                    packages: vec![pkg.name.clone()],
                    estimated_bytes: pkg.estimated_bytes,
                });
                continue;
            }
            let mut chunk_files: Vec<PathBuf> = Vec::new();
            let mut chunk_bytes: usize = 0;
            for file in &pkg.files {
                let arg_bytes = estimate_file_arg_bytes(file);
                if !chunk_files.is_empty() && chunk_bytes.saturating_add(arg_bytes) > limit {
                    batches.push(FmtBatch::DirectRustfmt {
                        package: pkg.name.clone(),
                        edition: pkg.edition.clone(),
                        files: std::mem::take(&mut chunk_files),
                        estimated_bytes: chunk_bytes,
                    });
                    chunk_bytes = 0;
                }
                chunk_files.push(file.clone());
                chunk_bytes = chunk_bytes.saturating_add(arg_bytes);
            }
            if !chunk_files.is_empty() {
                batches.push(FmtBatch::DirectRustfmt {
                    package: pkg.name.clone(),
                    edition: pkg.edition.clone(),
                    files: chunk_files,
                    estimated_bytes: chunk_bytes,
                });
            }
            continue;
        }

        if !current_pkgs.is_empty() && current_bytes.saturating_add(pkg.estimated_bytes) > limit {
            batches.push(FmtBatch::CargoFmt {
                packages: std::mem::take(&mut current_pkgs),
                estimated_bytes: current_bytes,
            });
            current_bytes = 0;
        }
        current_pkgs.push(pkg.name.clone());
        current_bytes = current_bytes.saturating_add(pkg.estimated_bytes);
    }

    if !current_pkgs.is_empty() {
        batches.push(FmtBatch::CargoFmt { packages: current_pkgs, estimated_bytes: current_bytes });
    }

    batches
}

fn find_rustfmt_config(workspace: &Path) -> Option<PathBuf> {
    let primary = workspace.join("rustfmt.toml");
    if primary.is_file() {
        return Some(primary);
    }
    let hidden = workspace.join(".rustfmt.toml");
    if hidden.is_file() {
        return Some(hidden);
    }
    None
}

/// Executes batched formatting (or `--check`) across the workspace described by `opts`.
pub fn run(opts: &Options) -> Result<Summary, String> {
    if opts.max_batch_bytes == 0 {
        return Err("max-batch-bytes must be greater than zero".to_string());
    }
    let packages = load_workspace_packages(&opts.workspace)?;
    let batches = plan_batches(&packages, opts.max_batch_bytes);
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let rustfmt = std::env::var("RUSTFMT").unwrap_or_else(|_| "rustfmt".into());
    let manifest_path = opts.workspace.join("Cargo.toml");
    let config_path = find_rustfmt_config(&opts.workspace);

    let mut all_passed = true;
    for batch in &batches {
        let status = match batch {
            FmtBatch::CargoFmt { packages: batch_pkgs, .. } => {
                let mut cmd = Command::new(&cargo);
                cmd.arg("fmt").arg("--manifest-path").arg(&manifest_path);
                if opts.check {
                    cmd.arg("--check");
                }
                for pkg in batch_pkgs {
                    cmd.arg("-p").arg(pkg);
                }
                cmd.status().map_err(|e| format!("cannot run cargo fmt: {e}"))?
            }
            FmtBatch::DirectRustfmt { edition, files, .. } => {
                let mut cmd = Command::new(&rustfmt);
                cmd.arg("--edition").arg(edition);
                if let Some(cfg) = &config_path {
                    cmd.arg("--config-path").arg(cfg);
                }
                if opts.check {
                    cmd.arg("--check");
                }
                cmd.args(files);
                cmd.status().map_err(|e| format!("cannot run rustfmt: {e}"))?
            }
        };
        if !status.success() {
            all_passed = false;
        }
    }

    Ok(Summary { passed: all_passed, packages: packages.len(), batches: batches.len() })
}

/// CLI entrypoint for `cargo xtask fmt`.
pub fn cli(args: &[String]) -> ExitCode {
    let mut opts = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--check" => opts.check = true,
            "--workspace" => match it.next() {
                Some(v) => opts.workspace = PathBuf::from(v),
                None => {
                    eprintln!("fmt: --workspace requires a directory\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--max-batch-bytes" => match it.next() {
                Some(v) => match v.parse::<usize>() {
                    Ok(n) if n > 0 => opts.max_batch_bytes = n,
                    _ => {
                        eprintln!("fmt: --max-batch-bytes requires a positive integer, got {v:?}\n{USAGE}");
                        return ExitCode::from(2);
                    }
                },
                None => {
                    eprintln!("fmt: --max-batch-bytes requires a positive integer\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    match run(&opts) {
        Ok(summary) if summary.passed => {
            println!("fmt: ok ({} packages, {} batches)", summary.packages, summary.batches);
            ExitCode::SUCCESS
        }
        Ok(_) => ExitCode::from(1),
        Err(e) => {
            eprintln!("fmt: {e}");
            ExitCode::from(2)
        }
    }
}
