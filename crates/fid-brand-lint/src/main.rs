//! `fid-brand-lint claim-lint --rules <file> [--bench-prefix <url>]... [--json] <paths>...`
//! (spec node OUTR.BRAND.LINT). Exit 0: no error findings; 1: error findings; 2: unreadable
//! or unparsable input or rule file, input over 5 MB, or bad usage.

use std::io::Write as _;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = fid_brand_lint::run(&args);
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&out.stdout).and_then(|()| stdout.flush()).is_err() {
        return ExitCode::from(2);
    }
    if !out.stderr.is_empty() {
        eprint!("{}", out.stderr);
    }
    ExitCode::from(out.code)
}
