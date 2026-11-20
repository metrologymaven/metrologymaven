//! `fid-design pkg-check --root <dir> [--json]`,
//! `fid-design pkg-diff --from <dir> --to <dir> [--json]`,
//! `fid-design princ-check [--file <md>] [--json]` and
//! `fid-design words-check --copy <jsonl> [--words <json>] [--json]`.
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
    file: Option<PathBuf>,
    copy: Option<PathBuf>,
    words: Option<PathBuf>,
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
            "--file" => out.file = Some(PathBuf::from(it.next()?)),
            "--copy" => out.copy = Some(PathBuf::from(it.next()?)),
            "--words" => out.words = Some(PathBuf::from(it.next()?)),
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
    let pkg_flags = args.root.is_some() || args.from.is_some() || args.to.is_some();
    let princ_flags = args.file.is_some() || args.copy.is_some() || args.words.is_some();
    match cmd.as_str() {
        "princ-check" if !pkg_flags && args.copy.is_none() && args.words.is_none() => {
            let file = args.file.clone().unwrap_or_else(|| PathBuf::from(fid_design::princ::DEFAULT_FILE));
            return match fid_design::princ::check(&file) {
                Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                Err(e) => fail(e),
            };
        }
        "words-check" if !pkg_flags && args.file.is_none() => {
            let Some(copy) = &args.copy else {
                return usage();
            };
            let words = args.words.clone().unwrap_or_else(|| PathBuf::from(fid_design::words::DEFAULT_WORDS));
            return match fid_design::words::check(copy, &words) {
                Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                Err(e) => fail(e),
            };
        }
        _ if princ_flags => return usage(),
        _ => {}
    }
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
