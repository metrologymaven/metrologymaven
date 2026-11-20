//! `dmis-classes (--out <file> | --check <file>)` (spec node `IO.DMIS.PARSE.TREE`).
//!
//! Generates the statement-class list `dmis-statement-classes.json` (schema
//! `fid.dmis-classes` v1) from the built-in descriptor registry. `--out` writes it;
//! `--check` fails if the committed file differs. The committed copy is
//! `docs/dmis/dmis-statement-classes.json`; the test `classes::committed_file_is_fresh` runs
//! the same check in CI.
//!
//! Exit codes: 0 success; 1 registry error, I/O error or stale file; 2 bad usage.

use fid_io_dmis::registry::Registry;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (check, path) = match args.as_slice() {
        [flag, path] if flag == "--out" => (false, path),
        [flag, path] if flag == "--check" => (true, path),
        _ => {
            eprintln!("usage: dmis-classes (--out <file> | --check <file>)");
            return ExitCode::from(2);
        }
    };
    let json = match Registry::builtin().and_then(|r| r.classes_json()) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("dmis-classes: {e}");
            return ExitCode::from(1);
        }
    };
    if check {
        match std::fs::read(path) {
            Ok(got) if got == json => ExitCode::SUCCESS,
            _ => {
                eprintln!("dmis-classes: {path} is stale; regenerate with --out");
                ExitCode::from(1)
            }
        }
    } else {
        match std::fs::write(path, &json) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("dmis-classes: {path}: {e}");
                ExitCode::from(1)
            }
        }
    }
}
