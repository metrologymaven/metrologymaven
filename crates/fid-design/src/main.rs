//! `fid-design pkg-check --root <dir> [--json]` and
//! `fid-design pkg-diff --from <dir> --to <dir> [--json]`.
//! Exit codes: 0 no findings, 1 findings, 2 unreadable package or bad usage.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: fid-design pkg-check --root <dir> [--json]\n       fid-design pkg-diff --from <dir> --to <dir> [--json]";

#[derive(Default)]
struct Args {
    root: Option<PathBuf>,
    from: Option<PathBuf>,
    to: Option<PathBuf>,
    json: bool,
}

fn parse_args(args: &[String]) -> Option<Args> {
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => out.json = true,
            "--root" => out.root = Some(PathBuf::from(it.next()?)),
            "--from" => out.from = Some(PathBuf::from(it.next()?)),
            "--to" => out.to = Some(PathBuf::from(it.next()?)),
            _ => return None,
        }
    }
    Some(out)
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn fail(e: impl std::fmt::Display) -> ExitCode {
    eprintln!("fid-design: error: {e}");
    ExitCode::from(2)
}

fn emit(bytes: &[u8]) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes).and_then(|()| out.flush()).map_err(|e| format!("cannot write the report: {e}"))
}

fn finish(json: Result<Vec<u8>, fid_canon::CanonError>, text: String, as_json: bool, code: u8) -> ExitCode {
    let body = if as_json {
        match json {
            Ok(mut b) => {
                b.push(b'\n');
                b
            }
            Err(e) => return fail(format!("report encoding: {}", e.code())),
        }
    } else {
        text.into_bytes()
    };
    match emit(&body) {
        Ok(()) => ExitCode::from(code),
        Err(e) => fail(e),
    }
}

fn main() -> ExitCode {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = all.first() else {
        return usage();
    };
    let Some(args) = parse_args(&all[1..]) else {
        return usage();
    };
    match (cmd.as_str(), &args.root, &args.from, &args.to) {
        ("pkg-check", Some(root), None, None) => match fid_design::pkg::check(root) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        ("pkg-diff", None, Some(from), Some(to)) => match fid_design::pkg::diff(from, to) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        _ => usage(),
    }
}
