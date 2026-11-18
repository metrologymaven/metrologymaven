//! `fid-outreach <command> ...`: Metrology Maven outreach tooling (domain OUTR).
//!
//! `metric-def check --defs <file> --sources <file> [--json]` (spec node `OUTR.COMM.METRIC.DEF`):
//! text mode prints one line per finding, then one `metric <id>` line per valid metric and a
//! summary; `--json` prints the canonical JSON report instead. Exits 0 with no findings, 1 with
//! any, and 2 on unreadable or unparsable input or bad usage.

use fid_outreach::metrics::def;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-outreach <command> [args]
commands:
  metric-def check --defs <file> --sources <file> [--json]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("metric-def"), Some("check")) => metric_def_check(args.get(2..).unwrap_or(&[])),
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
