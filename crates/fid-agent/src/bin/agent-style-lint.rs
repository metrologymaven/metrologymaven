//! `agent-style-lint`: the CI content linter of spec node `AGENT.SKILLS.STYLE`.
//!
//! Exit 0 when the curated guides, their Skills, their recipes and any extra Markdown follow
//! `agent/STYLE.md`; exit 1 with one `file:line: rule: detail` line per finding otherwise;
//! exit 2 on a usage error.

use fid_agent::{lint, StyleInputs};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: agent-style-lint --manifest <manifest.json> --skills <dir> \
[--catalog <agent/catalog.json>] [--recipes <agent/recipes>] [--denylist <agent/style-denylist.txt>] \
[--allow <agent/style-allow.txt>] [--prose <file-or-dir>]...";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut manifest = None;
    let mut skills = None;
    let mut catalog = PathBuf::from("agent/catalog.json");
    let mut recipes = PathBuf::from("agent/recipes");
    let mut denylist = PathBuf::from("agent/style-denylist.txt");
    let mut allow = None;
    let mut prose = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--manifest", Some(v)) => manifest = Some(PathBuf::from(v)),
            ("--skills", Some(v)) => skills = Some(PathBuf::from(v)),
            ("--catalog", Some(v)) => catalog = PathBuf::from(v),
            ("--recipes", Some(v)) => recipes = PathBuf::from(v),
            ("--denylist", Some(v)) => denylist = PathBuf::from(v),
            ("--allow", Some(v)) => allow = Some(PathBuf::from(v)),
            ("--prose", Some(v)) => prose.push(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(manifest), Some(skills)) = (manifest, skills) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let report = lint(&StyleInputs { manifest, catalog, skills, recipes, denylist, allow, prose });
    if report.findings.is_empty() {
        println!("agent-style-lint: ok, {} prose units", report.units);
        return ExitCode::SUCCESS;
    }
    for f in &report.findings {
        println!("{f}");
    }
    ExitCode::from(1)
}
