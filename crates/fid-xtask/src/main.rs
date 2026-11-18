//! `cargo xtask <task>` (alias in `.cargo/config.toml`).

mod pins;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo xtask layers [--workspace <dir>] [--map <layers.toml>]\n       \
                     cargo xtask datasets-check --catalogue <toml> --domains <file>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("layers") => layers(&args[1..]),
        Some("datasets-check") => datasets_check(&args),
        Some("pins") => pins::cli(&args[1..]),
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
