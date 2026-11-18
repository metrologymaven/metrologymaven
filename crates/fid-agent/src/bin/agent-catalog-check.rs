//! `agent-catalog-check`: the CI catalog check of spec node `AGENT.SKILLS.CATALOG`.
//!
//! Exit 0 and write the version index when the catalog is true against the manifest, the
//! profile index and the generated Skills; exit 1 with one finding per line otherwise;
//! exit 2 on a usage error.

use fid_agent::{check, Inputs};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: agent-catalog-check --manifest <manifest.json> --profiles <mcp-profiles.json> \
--skills <dir> [--catalog <agent/catalog.json>] [--recipes <agent/recipes>] [--out <catalog-index.json>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut manifest = None;
    let mut profiles = None;
    let mut skills = None;
    let mut catalog = PathBuf::from("agent/catalog.json");
    let mut recipes = PathBuf::from("agent/recipes");
    let mut out = PathBuf::from("catalog-index.json");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--manifest", Some(v)) => manifest = Some(PathBuf::from(v)),
            ("--profiles", Some(v)) => profiles = Some(PathBuf::from(v)),
            ("--skills", Some(v)) => skills = Some(PathBuf::from(v)),
            ("--catalog", Some(v)) => catalog = PathBuf::from(v),
            ("--recipes", Some(v)) => recipes = PathBuf::from(v),
            ("--out", Some(v)) => out = PathBuf::from(v),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(manifest), Some(profiles), Some(skills)) = (manifest, profiles, skills) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let inputs = Inputs { manifest, profiles, catalog, skills, recipes };
    match check(&inputs) {
        Ok(index) => {
            let written = index.to_canonical_bytes().map_err(|f| f.to_string()).and_then(|bytes| {
                std::fs::write(&out, bytes).map_err(|e| format!("bad_input: {}: {e}", out.display()))
            });
            match written {
                Ok(()) => {
                    println!("agent-catalog-check: ok, {} workflows, index {}", index.rows.len(), out.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    println!("{e}");
                    ExitCode::from(1)
                }
            }
        }
        Err(findings) => {
            for f in findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
    }
}
