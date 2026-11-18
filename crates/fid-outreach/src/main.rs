//! `fid-outreach plan-check --criteria <file> --apps <apps.toml> --plan <file> --reviews <dir>
//! [--base <file>] [--json]` (spec node `OUTR.LAUNCH.PLAN.FMT`). Project tooling, nearest class R0.
//!
//! Exits 0 with no findings, 1 with any finding, and 2 on bad usage or when an input cannot be
//! read or parsed.

use fid_outreach::launch_plan::{check, Inputs};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-outreach plan-check --criteria <file> --apps <apps.toml> --plan <file> \
                     --reviews <dir> [--base <file>] [--json]";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else { return usage() };
    if cmd != "plan-check" {
        return usage();
    }
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut json = false;
    let mut it = rest.iter();
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
    let inputs = Inputs {
        criteria: Path::new(criteria),
        apps: Path::new(apps),
        plan: Path::new(plan),
        reviews: Path::new(reviews),
        base: opts.get("--base").map(Path::new),
    };
    match check(&inputs) {
        Ok(report) => {
            if json {
                println!("{}", String::from_utf8_lossy(&report.to_json()));
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
                let bytes = fid_canon::to_canonical_bytes(&e.to_cvalue()).unwrap_or_default();
                println!("{}", String::from_utf8_lossy(&bytes));
            }
            eprintln!("plan-check: {e}");
            ExitCode::from(2)
        }
    }
}
