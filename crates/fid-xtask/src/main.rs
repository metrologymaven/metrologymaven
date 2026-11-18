//! `cargo xtask <task>` (alias in `.cargo/config.toml`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo xtask layers [--workspace <dir>] [--map <layers.toml>]\n       \
                     cargo xtask datasets-check --catalogue <toml> --domains <file>\n       \
                     cargo xtask bots-check --register <toml> --restricted <txt> --today <YYYY-MM-DD> \
                     [--review-export <json>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("layers") => layers(&args[1..]),
        Some("datasets-check") => datasets_check(&args),
        Some("bots-check") => bots_check(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
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

/// ECO.CI.BOTS.REG: exit 0 when the register passes, 1 on any finding, 2 on a usage error.
fn bots_check(args: &[String]) -> ExitCode {
    let mut opts: BTreeMap<&str, &str> = BTreeMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let known = matches!(a.as_str(), "--register" | "--restricted" | "--today" | "--review-export");
        match it.next() {
            Some(v) if known && !opts.contains_key(a.as_str()) => {
                opts.insert(a.as_str(), v.as_str());
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(register), Some(restricted), Some(today)) =
        (opts.get("--register"), opts.get("--restricted"), opts.get("--today"))
    else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let export = opts.get("--review-export").map(Path::new);
    match fid_xtask::bots::check(Path::new(register), Path::new(restricted), today, export) {
        Ok(findings) if findings.is_empty() => {
            println!("bots-check: ok");
            ExitCode::SUCCESS
        }
        Ok(findings) => {
            for f in findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            ExitCode::from(2)
        }
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
