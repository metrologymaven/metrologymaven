//! `fid-data datasets-check --catalogue <toml> --domains <file>` (spec node `ECO.DATA.CAT`).
//! `cargo xtask datasets-check ...` runs this binary.
//!
//! Prints one finding per line, sorted by entry ID and code, and exits 1 if there are
//! any; prints `datasets-check: ok` and exits 0 otherwise; exits 2 on bad usage.

use fid_data::cat::check_files;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-data datasets-check --catalogue <toml> --domains <file>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("datasets-check") => datasets_check(&args[1..]),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn datasets_check(args: &[String]) -> ExitCode {
    let mut catalogue: Option<PathBuf> = None;
    let mut domains: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--catalogue", Some(v)) => catalogue = Some(PathBuf::from(v)),
            ("--domains", Some(v)) => domains = Some(PathBuf::from(v)),
            _ => return usage(),
        }
    }
    let (Some(catalogue), Some(domains)) = (catalogue, domains) else {
        return usage();
    };
    let findings = check_files(&catalogue, &domains);
    if findings.is_empty() {
        println!("datasets-check: ok");
        return ExitCode::SUCCESS;
    }
    for f in &findings {
        println!("{f}");
    }
    ExitCode::from(1)
}
