//! `fid-outreach <command> ...`: Metrology Maven outreach tooling (domain OUTR).
//!
//! `metric-def check --defs <file> --sources <file> [--json]` (spec node `OUTR.COMM.METRIC.DEF`):
//! text mode prints one line per finding, then one `metric <id>` line per valid metric and a
//! summary; `--json` prints the canonical JSON report instead. Exits 0 with no findings, 1 with
//! any, and 2 on unreadable or unparsable input or bad usage.
//!
//! `plan-check --criteria <file> --apps <apps.toml> --plan <file> --reviews <dir> [--base <file>]
//! [--json]` (spec node `OUTR.LAUNCH.PLAN.FMT`): validates the milestone criteria file, the D-017
//! rule, plan-document dates and announced criteria. Exits 0 with no findings, 1 with any, and 2
//! when an input cannot be read or parsed, or on bad usage.
//!
//! `qa a11y --root <dir> --tokens <file> --out <file> [--config <file>]` (spec node
//! `OUTR.SITE.QA.A11Y`): writes the report to `--out`, prints one line per unsuppressed
//! violation, and exits 0 when there is none, 1 when there is any, and 2 on unreadable or
//! unparsable input or bad usage.

use fid_outreach::launch_plan;
use fid_outreach::metrics::def;
use fid_outreach::qa::a11y::{self, Options};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: fid-outreach <command> [args]
commands:
  metric-def check --defs <file> --sources <file> [--json]
  plan-check --criteria <file> --apps <apps.toml> --plan <file> --reviews <dir> [--base <file>] [--json]
  qa a11y --root <dir> --tokens <file> --out <file> [--config <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("metric-def"), Some("check")) => metric_def_check(args.get(2..).unwrap_or(&[])),
        (Some("plan-check"), _) => plan_check(args.get(1..).unwrap_or(&[])),
        (Some("qa"), Some("a11y")) => qa_a11y(args.get(2..).unwrap_or(&[])),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn emit(bytes: &[u8]) {
    let mut out = std::io::stdout().lock();
    // A closed stdout is not worth a different exit code; the exit status carries the result.
    let _ = out.write_all(bytes).and_then(|()| out.write_all(b"\n"));
}

fn metric_def_check(args: &[String]) -> ExitCode {
    let mut defs: Option<PathBuf> = None;
    let mut sources: Option<PathBuf> = None;
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--defs" => match it.next() {
                Some(v) => defs = Some(PathBuf::from(v)),
                None => return usage(),
            },
            "--sources" => match it.next() {
                Some(v) => sources = Some(PathBuf::from(v)),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    let (Some(defs), Some(sources)) = (defs, sources) else {
        return usage();
    };
    match def::check_files(&defs, &sources) {
        Ok(report) => {
            if json {
                emit(&report.to_json());
            } else {
                for f in &report.findings {
                    println!("{f}");
                }
                for m in &report.metrics {
                    println!("metric {}", m.id);
                }
                println!(
                    "metric-def check: {} metric(s), {} source(s), {} finding(s)",
                    report.metrics.len(),
                    report.sources.len(),
                    report.findings.len()
                );
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            if json {
                emit(&def::error_json(&e));
            }
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn plan_check(args: &[String]) -> ExitCode {
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            flag @ ("--criteria" | "--apps" | "--plan" | "--reviews" | "--base") => {
                let Some(v) = it.next() else { return usage() };
                if opts.insert(flag, v.as_str()).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let (Some(criteria), Some(apps), Some(plan), Some(reviews)) =
        (opts.get("--criteria"), opts.get("--apps"), opts.get("--plan"), opts.get("--reviews"))
    else {
        return usage();
    };
    let inputs = launch_plan::Inputs {
        criteria: Path::new(criteria),
        apps: Path::new(apps),
        plan: Path::new(plan),
        reviews: Path::new(reviews),
        base: opts.get("--base").map(Path::new),
    };
    match launch_plan::check(&inputs) {
        Ok(report) => {
            if json {
                emit(&report.to_json());
            } else {
                print!("{}", report.render());
            }
            if report.findings.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            if json {
                emit(&fid_canon::to_canonical_bytes(&e.to_cvalue()).unwrap_or_default());
            }
            eprintln!("plan-check: {e}");
            ExitCode::from(2)
        }
    }
}

fn qa_a11y(args: &[String]) -> ExitCode {
    let mut root: Option<PathBuf> = None;
    let mut tokens: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut config: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--root", Some(v)) => root = Some(PathBuf::from(v)),
            ("--tokens", Some(v)) => tokens = Some(PathBuf::from(v)),
            ("--out", Some(v)) => out = Some(PathBuf::from(v)),
            ("--config", Some(v)) => config = Some(PathBuf::from(v)),
            _ => return usage(),
        }
    }
    let (Some(root), Some(tokens), Some(out)) = (root, tokens, out) else {
        return usage();
    };
    match a11y::run(&Options { root, tokens, config }) {
        Ok(report) => {
            let bytes = match report.to_json() {
                Ok(b) => b,
                Err(m) => {
                    eprintln!("A11Y-IO({}): {m}", out.display());
                    return ExitCode::from(2);
                }
            };
            if let Err(e) = std::fs::write(&out, &bytes) {
                eprintln!("A11Y-IO({}): {e}", out.display());
                return ExitCode::from(2);
            }
            for v in &report.violations {
                println!("{v}");
            }
            println!(
                "qa a11y: {} page(s), {} violation(s), {} suppressed, {} unmatched suppression(s)",
                report.pages,
                report.violations.len(),
                report.suppressed.len(),
                report.unmatched.len()
            );
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            eprintln!("{e}");
            if let Err(w) = std::fs::write(&out, a11y::error_json(&e)) {
                eprintln!("A11Y-IO({}): {w}", out.display());
            }
            ExitCode::from(2)
        }
    }
}
