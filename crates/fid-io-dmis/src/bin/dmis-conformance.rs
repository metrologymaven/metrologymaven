//! `dmis-conformance --decl <toml> --tests <inventory> (--out-dir <dir> | --check <dir>)`
//! (spec node `IO.DMIS.CONF`).
//!
//! `--out-dir` writes `dmis-conformance.json` and `dmis-conformance.md` into the directory.
//! `--check` regenerates in memory and fails if the committed files in the directory differ
//! (the CI staleness check, AC-4). The inventory is the output of `cargo test -- --list`.
//!
//! Exit codes: 0 success; 1 generation error (`malformed`, `conformance_unproven`, `io`) or
//! stale files; 2 bad usage.

use fid_io_dmis::conf::{generate_files, stale_files, write_outputs};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: dmis-conformance --decl <toml> --tests <inventory> (--out-dir <dir> | --check <dir>)";

enum Mode {
    Write(PathBuf),
    Check(PathBuf),
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut decl: Option<PathBuf> = None;
    let mut tests: Option<PathBuf> = None;
    let mut mode: Option<Mode> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--decl", Some(v)) if decl.is_none() => decl = Some(PathBuf::from(v)),
            ("--tests", Some(v)) if tests.is_none() => tests = Some(PathBuf::from(v)),
            ("--out-dir", Some(v)) if mode.is_none() => mode = Some(Mode::Write(PathBuf::from(v))),
            ("--check", Some(v)) if mode.is_none() => mode = Some(Mode::Check(PathBuf::from(v))),
            _ => return usage(),
        }
    }
    let (Some(decl), Some(tests), Some(mode)) = (decl, tests, mode) else {
        return usage();
    };
    let out = match generate_files(&decl, &tests) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("dmis-conformance: {e}");
            return ExitCode::from(1);
        }
    };
    match mode {
        Mode::Write(dir) => match write_outputs(&out, &dir) {
            Ok(()) => {
                println!("dmis-conformance: wrote {}", dir.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("dmis-conformance: {e}");
                ExitCode::from(1)
            }
        },
        Mode::Check(dir) => {
            let stale = stale_files(&out, &dir);
            if stale.is_empty() {
                println!("dmis-conformance: up to date");
                return ExitCode::SUCCESS;
            }
            for name in stale {
                eprintln!("dmis-conformance: stale: {} differs from a fresh generation", dir.join(name).display());
            }
            ExitCode::from(1)
        }
    }
}
