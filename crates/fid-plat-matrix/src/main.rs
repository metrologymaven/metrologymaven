//! `plat-matrix validate <file>` (spec node `PLAT.MATRIX.FILE`).
//!
//! Prints one diagnostic per line and then `<rows> rows, <errors> errors`; exits 0 when
//! there are no errors, 1 when there are, and 2 on bad usage or an unreadable file.

use std::io::Read;
use std::process::ExitCode;

const USAGE: &str = "usage: plat-matrix validate <file>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, file] if cmd == "validate" => validate(file),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn validate(path: &str) -> ExitCode {
    // Read at most one byte past the limit so an oversized file is reported, not loaded.
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|f| f.take(fid_plat_matrix::MAX_BYTES as u64 + 1).read_to_end(&mut bytes));
    if let Err(e) = read {
        eprintln!("plat-matrix: cannot read {path}: {e}");
        return ExitCode::from(2);
    }
    let v = fid_plat_matrix::validate(&bytes);
    print!("{}", v.render());
    if v.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
