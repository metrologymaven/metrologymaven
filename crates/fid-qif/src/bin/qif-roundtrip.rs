//! `qif-roundtrip --manifest <path>`: the QIF round-trip gate (IO.QIF.NORM AC-6).
//!
//! Prints the JSON report on stdout. Exit codes: 0 every selected file is equal under
//! qif-norm after XML → JSON → XML; 1 some file is not (or nothing was selected);
//! 2 usage error or unreadable manifest. Uses the stub projection until IO.QIF.JSON
//! provides the real one.

use fid_qif::roundtrip::{run_manifest, StubProjection};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let manifest = match args.as_slice() {
        [flag, path] if flag == "--manifest" => PathBuf::from(path),
        _ => {
            eprintln!("usage: qif-roundtrip --manifest <corpus manifest .toml>");
            return ExitCode::from(2);
        }
    };
    match run_manifest(&manifest, &StubProjection) {
        Ok(report) => {
            let mut out = std::io::stdout().lock();
            if out.write_all(report.to_json().as_bytes()).is_err() {
                return ExitCode::from(2);
            }
            if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
