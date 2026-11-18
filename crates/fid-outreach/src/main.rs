//! `fid-outreach qa a11y --root <dir> --tokens <file> --out <file> [--config <file>]`
//! (spec node `OUTR.SITE.QA.A11Y`).
//!
//! Writes the report to `--out`, prints one line per unsuppressed violation, and exits 0 when
//! there is none, 1 when there is any, and 2 on unreadable or unparsable input or bad usage.

use fid_outreach::qa::a11y::{self, Options};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-outreach qa a11y --root <dir> --tokens <file> --out <file> [--config <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("qa"), Some("a11y")) => qa_a11y(args.get(2..).unwrap_or(&[])),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
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
