//! CLI entry point for `fid-vocab`:
//! - `fid-vocab lint --rules <file> --units <jsonl> [--json]`
//! - `fid-vocab test-rules --rules <file>`
//! - `fid-vocab catalog-units --export <json> [--suppressions <toml>]`
//! - `fid-vocab catalog-lint --export <json> --rules <toml> [--suppressions <toml>] [--json]`
//!
//! Exit codes: 0 no error findings (or all examples behave / units emitted), 1 error findings
//! (or an example misbehaves), 2 unreadable or unparsable input, invalid suppression, or bad usage.

use fid_vocab::{
    format_units_jsonl, lint, load_catalog_units, parse_units, read_limited, test_rules, RuleSet, MAX_UNITS_BYTES,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: fid-vocab lint --rules <file> --units <jsonl> [--json]\n\
                     \x20      fid-vocab test-rules --rules <file>\n\
                     \x20      fid-vocab catalog-units --export <json> [--suppressions <toml>]\n\
                     \x20      fid-vocab catalog-lint --export <json> --rules <toml> [--suppressions <toml>] [--json]";

struct Args {
    rules: Option<PathBuf>,
    units: Option<PathBuf>,
    export: Option<PathBuf>,
    suppressions: Option<PathBuf>,
    json: bool,
}

fn parse_args(args: &[String]) -> Option<Args> {
    let mut out = Args { rules: None, units: None, export: None, suppressions: None, json: false };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => out.json = true,
            "--rules" => out.rules = Some(PathBuf::from(it.next()?)),
            "--units" => out.units = Some(PathBuf::from(it.next()?)),
            "--export" => out.export = Some(PathBuf::from(it.next()?)),
            "--suppressions" => out.suppressions = Some(PathBuf::from(it.next()?)),
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
        "lint" if args.export.is_none() && args.suppressions.is_none() => run_lint(&args),
        "test-rules" if args.units.is_none() && args.export.is_none() && args.suppressions.is_none() && !args.json => {
            run_test_rules(&args)
        }
        "catalog-units" if args.rules.is_none() && args.units.is_none() && !args.json => run_catalog_units(&args),
        "catalog-lint" if args.units.is_none() => run_catalog_lint(&args),
        _ => usage(),
    }
}

fn load_checked_rules(rules_path: &Path) -> Result<RuleSet, ExitCode> {
    let rules = match RuleSet::load(rules_path) {
        Ok(r) => r,
        Err(e) => return Err(fail(e)),
    };
    // The engine refuses a rule file whose examples are missing or misbehave.
    let problems = test_rules(&rules);
    if !problems.is_empty() {
        for p in &problems {
            eprintln!("fid-vocab: {p}");
        }
        return Err(fail(format!(
            "rule file {} refused: {} example problem(s); run `fid-vocab test-rules`",
            rules_path.display(),
            problems.len()
        )));
    }
    Ok(rules)
}

fn emit_report(report: &fid_vocab::Report, json: bool) -> ExitCode {
    let mut body = if json {
        match report.to_json() {
            Ok(bytes) => bytes,
            Err(e) => return fail(format!("report encoding: {}", e.code())),
        }
    } else {
        report.to_text().into_bytes()
    };
    if json {
        body.push(b'\n');
    }
    match emit(&body) {
        Ok(()) => ExitCode::from(report.exit_code()),
        Err(e) => fail(e),
    }
}

fn run_lint(args: &Args) -> ExitCode {
    let (Some(rules_path), Some(units_path)) = (&args.rules, &args.units) else {
        return usage();
    };
    let rules = match load_checked_rules(rules_path) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let units = match read_limited(units_path, MAX_UNITS_BYTES, "unit stream").and_then(|b| parse_units(&b)) {
        Ok(u) => u,
        Err(e) => return fail(e),
    };
    let report = lint(&rules, &units);
    emit_report(&report, args.json)
}

fn run_catalog_units(args: &Args) -> ExitCode {
    let Some(export_path) = &args.export else {
        return usage();
    };
    let units = match load_catalog_units(export_path, args.suppressions.as_deref()) {
        Ok(u) => u,
        Err(e) => return fail(e),
    };
    let body = match format_units_jsonl(&units) {
        Ok(b) => b,
        Err(e) => return fail(e),
    };
    match emit(&body) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

fn run_catalog_lint(args: &Args) -> ExitCode {
    let (Some(export_path), Some(rules_path)) = (&args.export, &args.rules) else {
        return usage();
    };
    let rules = match load_checked_rules(rules_path) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let units = match load_catalog_units(export_path, args.suppressions.as_deref()) {
        Ok(u) => u,
        Err(e) => return fail(e),
    };
    let report = lint(&rules, &units);
    emit_report(&report, args.json)
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
    let mut out = format!("fid-vocab: rules version {}\n", rules.version);
    for p in &problems {
        out.push_str(p);
        out.push('\n');
    }
    if problems.is_empty() {
        out.push_str(&format!("{} rule(s): all examples behave\n", rules.rules.len()));
    }
    match emit(out.as_bytes()) {
        Ok(()) if problems.is_empty() => ExitCode::SUCCESS,
        Ok(()) => ExitCode::from(1),
        Err(e) => fail(e),
    }
}
