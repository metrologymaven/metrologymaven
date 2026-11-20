//! `fid-design pkg-check --root <dir> [--json]`,
//! `fid-design pkg-diff --from <dir> --to <dir> [--json]`,
//! `fid-design tokens-check --root <dir> [--json]`,
//! `fid-design status-check --root <dir> [--json]`,
//! `fid-design palettes-check --root <dir> [--baseline <digests>] [--json]`,
//! `fid-design princ-check [--file <md>] [--json]` and
//! `fid-design words-check --copy <jsonl> [--words <json>] [--json]` and
//! `fid-design review-check --changed <file> --root <design dir> [--base <repo>] [--pr <N>]
//! [--deny-names <file>] [--json]` or `review-check --all --root <design dir> [--base <repo>] ..`.
//! Exit codes: 0 no findings, 1 findings, 2 unreadable package or bad usage.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: fid-design pkg-check --root <dir> [--json]\n       fid-design pkg-diff --from <dir> --to <dir> [--json]\n       fid-design tokens-check --root <design dir> [--json]\n       fid-design status-check --root <design dir> [--json]\n       fid-design palettes-check --root <design dir> [--baseline <digests>] [--json]\n       fid-design princ-check [--file <md>] [--json]\n       fid-design words-check --copy <jsonl> [--words <json>] [--json]\n       fid-design review-check --changed <file> --root <design dir> [--base <repo>] [--pr <N>] [--deny-names <file>] [--json]\n       fid-design review-check --all --root <design dir> [--base <repo>] [--deny-names <file>] [--json]";

#[derive(Default)]
struct Args {
    root: Option<PathBuf>,
    from: Option<PathBuf>,
    to: Option<PathBuf>,
    file: Option<PathBuf>,
    copy: Option<PathBuf>,
    words: Option<PathBuf>,
    changed: Option<PathBuf>,
    deny_names: Option<PathBuf>,
    base: Option<PathBuf>,
    baseline: Option<PathBuf>,
    pr: Option<u64>,
    all: bool,
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
            "--changed" => out.changed = Some(PathBuf::from(it.next()?)),
            "--deny-names" => out.deny_names = Some(PathBuf::from(it.next()?)),
            "--base" => out.base = Some(PathBuf::from(it.next()?)),
            "--baseline" => out.baseline = Some(PathBuf::from(it.next()?)),
            "--pr" => {
                let n: u64 = it.next()?.parse().ok()?;
                if n == 0 {
                    return None;
                }
                out.pr = Some(n);
            }
            "--all" => out.all = true,
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
    if cmd == "review-check" {
        let mode_ok = if args.all { args.changed.is_none() && args.pr.is_none() } else { args.changed.is_some() };
        let Some(root) = args.root.clone() else {
            return usage();
        };
        if !mode_ok || princ_flags || args.from.is_some() || args.to.is_some() || args.baseline.is_some() {
            return usage();
        }
        let o = fid_design::review::Options {
            changed: args.changed.clone(),
            root,
            base: args.base.clone(),
            deny: args.deny_names.clone(),
            pr: args.pr,
            all: args.all,
        };
        return match fid_design::review::check(&o) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        };
    }
    if args.changed.is_some()
        || args.deny_names.is_some()
        || args.base.is_some()
        || args.pr.is_some()
        || args.all
        || (args.baseline.is_some() && cmd != "palettes-check")
    {
        return usage();
    }
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
        ("tokens-check", Some(root), None, None) => match fid_design::tokens::check(root) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        ("status-check", Some(root), None, None) => match fid_design::status::check(root) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        ("palettes-check", Some(root), None, None) => match fid_design::palettes::check(root, args.baseline.as_deref())
        {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        _ => usage(),
    }
}
