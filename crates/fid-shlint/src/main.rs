//! `fid-shlint --crate-dir <dir> --metadata <cargo-metadata.json> [--package <name>]`
//! (spec node VIZ.RENDER.FRAME.SHLINT). Exit 0: no findings; 1: findings; 2: unreadable or
//! unparsable input, an input over its limit, or bad usage.

use std::io::Write as _;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = fid_shlint::run(&args);
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(out.stdout.as_bytes()).and_then(|()| stdout.flush()).is_err() {
        return ExitCode::from(2);
    }
    if !out.stderr.is_empty() {
        eprint!("{}", out.stderr);
    }
    ExitCode::from(out.code)
}
