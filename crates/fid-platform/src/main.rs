//! `fid-platform plat-matrix validate <file>` (spec node `PLAT.MATRIX.FILE`) and
//! `fid-platform plat-hw validate [--file <path>]` (spec node `PLAT.HW.PROFILE`).
//! `cargo xtask plat-matrix ...` and `cargo xtask plat-hw ...` run this binary.
//!
//! `plat-matrix validate` prints one diagnostic per line and then `<rows> rows, <errors> errors`.
//! `plat-hw validate` prints the profile names, one per line, when the file is valid, and
//! one diagnostic per line otherwise. Both exit 0 when there are no errors, 1 when there
//! are, and 2 on bad usage or an unreadable file.

use fid_platform::{hw_profile, matrix};
use std::io::Read;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-platform plat-matrix validate <file>\n       \
                     fid-platform plat-hw validate [--file <path>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [group, cmd, file] if group == "plat-matrix" && cmd == "validate" => validate(file),
        [group, cmd] if group == "plat-hw" && cmd == "validate" => validate_hw(hw_profile::DEFAULT_PATH),
        [group, cmd, flag, file] if group == "plat-hw" && cmd == "validate" && flag == "--file" => validate_hw(file),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Reads at most `max + 1` bytes so an oversized file is reported, not loaded.
fn read_capped(path: &str, max: usize, cmd: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(max as u64 + 1).read_to_end(&mut bytes));
    match read {
        Ok(_) => Some(bytes),
        Err(e) => {
            eprintln!("{cmd}: cannot read {path}: {e}");
            None
        }
    }
}

fn validate(path: &str) -> ExitCode {
    let Some(bytes) = read_capped(path, matrix::MAX_BYTES, "plat-matrix validate") else {
        return ExitCode::from(2);
    };
    let v = matrix::validate(&bytes);
    print!("{}", v.render());
    if v.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn validate_hw(path: &str) -> ExitCode {
    let Some(bytes) = read_capped(path, hw_profile::MAX_BYTES, "plat-hw validate") else {
        return ExitCode::from(2);
    };
    match hw_profile::validate(&bytes) {
        Ok(p) => {
            for name in p.profiles.keys() {
                println!("{name}");
            }
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for e in errors {
                println!("{e}");
            }
            ExitCode::from(1)
        }
    }
}
