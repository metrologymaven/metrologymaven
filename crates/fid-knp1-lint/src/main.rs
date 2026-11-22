//! Binary entry point for `fid-knp1-lint` (spec node `KRN.NUM.LINT.CLI`).

use std::io::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (code, stdout, stderr) = fid_knp1_lint::cli::run_cli(&args);
    if !stdout.is_empty() {
        let _ = std::io::stdout().write_all(stdout.as_bytes());
    }
    if !stderr.is_empty() {
        let _ = std::io::stderr().write_all(stderr.as_bytes());
    }
    std::process::exit(code);
}
