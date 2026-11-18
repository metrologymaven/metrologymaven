//! `fid-vocab lint --rules <file> --units <jsonl> [--json]` and `fid-vocab test-rules --rules <file>`.
//! Exit codes: 0 no error findings (or all examples behave), 1 error findings (or an example
//! misbehaves), 2 unreadable or unparsable rule file or input, or bad usage.

use fid_vocab::{lint, parse_units, read_limited, test_rules, RuleSet, MAX_UNITS_BYTES};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: fid-vocab lint --rules <file> --units <jsonl> [--json]\n       fid-vocab test-rules --rules <file>";

struct Args {
    rules: Option<PathBuf>,
    units: Option<PathBuf>,
    json: bool,
}

fn parse_args(args: &[String]) -> Option<Args> {
    let mut out = Args { rules: None, units: None, json: false };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => out.json = true,
            "--rules" => out.rules = Some(PathBuf::from(it.next()?)),
            "--units" => out.units = Some(PathBuf::from(it.next()?)),
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
    eprintln!("fid-vocab: error: {e}");
    ExitCode::from(2)
}

/// Writes `bytes` to locked stdout and flushes; a write error (for example a closed pipe)
/// is reported by the caller with exit 2.
fn emit(bytes: &[u8]) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes).and_then(|()| out.flush()).map_err(|e| format!("cannot write the report: {e}"))
}

fn main() -> ExitCode {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = all.first() else {
        return usage();
    };
    let Some(args) = parse_args(&all[1..]) else {
        return usage();
    };
    match cmd.as_str() {
        "lint" => run_lint(&args),
        "test-rules" if args.units.is_none() && !args.json => run_test_rules(&args),
        _ => usage(),
    }
}

fn run_lint(args: &Args) -> ExitCode {
    let (Some(rules_path), Some(units_path)) = (&args.rules, &args.units) else {
        return usage();
    };
    let rules = match RuleSet::load(rules_path) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    // The engine refuses a rule file whose examples are missing or misbehave.
    let problems = test_rules(&rules);
    if !problems.is_empty() {
        for p in &problems {
            eprintln!("fid-vocab: {p}");
        }
        return fail(format!(
            "rule file {} refused: {} example problem(s); run `fid-vocab test-rules`",
            rules_path.display(),
            problems.len()
        ));
    }
    let units = match read_limited(units_path, MAX_UNITS_BYTES, "unit stream").and_then(|b| parse_units(&b)) {
        Ok(u) => u,
        Err(e) => return fail(e),
    };
    let report = lint(&rules, &units);
    let mut body = if args.json {
        match report.to_json() {
            Ok(bytes) => bytes,
            Err(e) => return fail(format!("report encoding: {}", e.code())),
        }
    } else {
        report.to_text().into_bytes()
    };
    if args.json {
        body.push(b'
');
    }
    match emit(&body) {
        Ok(()) => ExitCode::from(report.exit_code()),
        Err(e) => fail(e),
    }
}

fn run_test_rules(args: &Args) -> ExitCode {
    let Some(rules_path) = &args.rules else {
        return usage();
    };
    let rules = match RuleSet::load(rules_path) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let problems = test_rules(&rules);
    let mut out = format!("fid-vocab: rules version {}
", rules.version);
    for p in &problems {
        out.push_str(p);
        out.push('
');
    }
    if problems.is_empty() {
        out.push_str(&format!("{} rule(s): all examples behave
", rules.rules.len()));
    }
    match emit(out.as_bytes()) {
        Ok(()) if problems.is_empty() => ExitCode::SUCCESS,
        Ok(()) => ExitCode::from(1),
        Err(e) => fail(e),
    }
}
