//! `fid-platform plat-matrix validate <file>` (spec node `PLAT.MATRIX.FILE`) and
//! `fid-platform plat-hw validate [--file <path>]` (spec node `PLAT.HW.PROFILE`),
//! `fid-platform plat-hw validate-suite [--file <path>]` and
//! `fid-platform plat-hw result-schema` (spec node `PLAT.HW.SCHEMA`).
//! `cargo xtask plat-matrix ...` and `cargo xtask plat-hw ...` run this binary.
//!
//! `plat-matrix validate` prints one diagnostic per line and then `<rows> rows, <errors> errors`.
//! `plat-hw validate` prints the profile names, one per line, when the file is valid, and
//! one diagnostic per line otherwise. `plat-hw validate-suite` does the same for a benchmark
//! suite manifest, printing workload IDs. All three exit 0 when there are no errors, 1 when
//! there are, and 2 on bad usage or an unreadable file. `plat-hw result-schema` writes the
//! result JSON Schema to `platform/bench-result.schema.json` (exit 0, or 2 if it cannot).

use fid_platform::{bench_schema, hw_profile, matrix};
use std::io::Read;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-platform plat-matrix validate <file>\n       \
                     fid-platform plat-hw validate [--file <path>]\n       \
                     fid-platform plat-hw validate-suite [--file <path>]\n       \
                     fid-platform plat-hw result-schema";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [group, cmd, file] if group == "plat-matrix" && cmd == "validate" => validate(file),
        [group, cmd] if group == "plat-hw" && cmd == "validate" => validate_hw(hw_profile::DEFAULT_PATH),
        [group, cmd, flag, file] if group == "plat-hw" && cmd == "validate" && flag == "--file" => validate_hw(file),
        [group, cmd] if group == "plat-hw" && cmd == "validate-suite" => validate_suite(bench_schema::SUITE_DEFAULT_PATH),
        [group, cmd, flag, file] if group == "plat-hw" && cmd == "validate-suite" && flag == "--file" => {
            validate_suite(file)
        }
        [group, cmd] if group == "plat-hw" && cmd == "result-schema" => result_schema(),
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

fn validate_suite(path: &str) -> ExitCode {
    let Some(bytes) = read_capped(path, bench_schema::MAX_BYTES, "plat-hw validate-suite") else {
        return ExitCode::from(2);
    };
    match bench_schema::validate_suite(&bytes) {
        Ok(suite) => {
            for w in &suite.workloads {
                println!("{}", w.id);
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

fn result_schema() -> ExitCode {
    let path = bench_schema::RESULT_SCHEMA_PATH;
    let written = bench_schema::result_json_schema_bytes()
        .map_err(|e| e.code().to_string())
        .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()));
    match written {
        Ok(()) => {
            println!("wrote {path}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("plat-hw result-schema: cannot write {path}: {e}");
            ExitCode::from(2)
        }
    }
}
