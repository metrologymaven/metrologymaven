//! `cargo xtask <task>` (alias in `.cargo/config.toml`).

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask layers [--workspace <dir>] [--map <layers.toml>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("layers") => layers(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
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
