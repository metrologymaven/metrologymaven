//! CLI binary for acceptance report queries (`fid-accept show-report`, `FLOW.RUNNER.VERDICT`).

use std::io::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (code, stdout, stderr) = fid_accept::report::run_cli(&args);
    let _ = std::io::stdout().write_all(stdout.as_bytes());
    let _ = std::io::stderr().write_all(stderr.as_bytes());
    std::process::exit(code);
}
