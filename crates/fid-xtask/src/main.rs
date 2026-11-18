//! `cargo xtask <task>` (alias in `.cargo/config.toml`).

mod pins;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo xtask layers [--workspace <dir>] [--map <layers.toml>]\n       \
                     cargo xtask datasets-check --catalogue <toml> --domains <file>\n       \
                     cargo xtask plat-matrix validate <file>\n       \
                     cargo xtask personas-check [--personas <json>] [--apps <toml>]";
const LINT_USAGE: &str =
    "usage: cargo xtask lint-crate-graph [--workspace <dir>] [--allowlist <file>] [--writers <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("layers") => layers(&args[1..]),
        Some("lint-crate-graph") => lint_crate_graph(&args[1..]),
        Some("datasets-check") => datasets_check(&args),
        Some("plat-matrix") => plat_matrix(&args),
        Some("pins") => pins::cli(&args[1..]),
        Some("personas-check") => personas_check(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            eprintln!("{LINT_USAGE}");
            ExitCode::from(2)
        }
    }
}

/// ECO.DATA.CAT: runs the `fid-data` binary. `fid-data` is a `tool` crate, and the layer
/// rule forbids depending on one, so the wrapper starts it through Cargo instead.
fn datasets_check(args: &[String]) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let status = Command::new(cargo)
        .args(["run", "-q", "--manifest-path", manifest, "-p", "fid-data", "--bin", "fid-data", "--"])
        .args(args)
        .status();
    match status {
        Ok(s) => ExitCode::from(s.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(1)),
        Err(e) => {
            println!("DATA-IO -: cannot run cargo: {e}");
            ExitCode::from(1)
        }
    }
}

/// PLAT.MATRIX.FILE: runs `fid-platform plat-matrix ...` through Cargo, for the same layer
/// reason as `datasets-check` (`fid-platform` is a `tool` crate).
fn plat_matrix(args: &[String]) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let status = Command::new(cargo)
        .args(["run", "-q", "--manifest-path", manifest, "-p", "fid-platform", "--bin", "fid-platform", "--"])
        .args(args)
        .status();
    match status {
        Ok(s) => ExitCode::from(s.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(2)),
        Err(e) => {
            eprintln!("plat-matrix: cannot run cargo: {e}");
            ExitCode::from(2)
        }
    }
}

/// FLOW.PERSONA.PERS: checks the persona definitions file against the app list and the
/// built-in governance profile names. Exit 0: clean; 1: findings; 2: usage error.
fn personas_check(args: &[String]) -> ExitCode {
    let mut personas = PathBuf::from("acceptance/personas/personas.json");
    let mut apps = PathBuf::from("apps.toml");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--personas", Some(v)) => personas = PathBuf::from(v),
            ("--apps", Some(v)) => apps = PathBuf::from(v),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let report = fid_accept::persona::check_files(&personas, &apps);
    for f in &report.findings {
        println!("{f}");
    }
    if report.is_ok() {
        println!("personas-check: {} personas", report.set.personas.len());
        ExitCode::SUCCESS
    } else {
        println!("personas-check: {} findings", report.findings.len());
        ExitCode::from(1)
    }
}

fn layers(args: &[String]) -> ExitCode {
    let mut workspace = PathBuf::from(".");
    let mut map: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--workspace", Some(v)) => workspace = PathBuf::from(v),
            ("--map", Some(v)) => map = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let map = map.unwrap_or_else(|| workspace.join("layers.toml"));
    match fid_xtask::run(&workspace, &map) {
        Ok(findings) if findings.is_empty() => {
            println!("layers: ok");
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

/// `lint-crate-graph` (CORE.BUS.LINT). Exit 0: clean; 1: findings; 2: usage, allowlist or
/// metadata error.
fn lint_crate_graph(args: &[String]) -> ExitCode {
    let mut workspace = PathBuf::from(".");
    let mut allowlist: Option<PathBuf> = None;
    let mut writers: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--workspace", Some(v)) => workspace = PathBuf::from(v),
            ("--allowlist", Some(v)) => allowlist = Some(PathBuf::from(v)),
            ("--writers", Some(v)) => writers = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{LINT_USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let allowlist = allowlist.unwrap_or_else(|| workspace.join(fid_xtask::crate_graph::ALLOWLIST_FILE));
    let writers = writers.unwrap_or_else(|| workspace.join(fid_xtask::crate_graph::WRITERS_FILE));
    match fid_xtask::crate_graph::run_with_warnings(&workspace, &allowlist, &writers) {
        Ok((findings, warnings)) => {
            for w in warnings {
                eprintln!("{w}");
            }
            if findings.is_empty() {
                println!("lint-crate-graph: ok");
                return ExitCode::SUCCESS;
            }
            for f in findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
