//! Dev binary `conic-xtest` (spec node `KRN.SOLV.CONIC.XTEST`).
//!
//! Usage:
//!   conic-xtest run --fixtures <dir> [--out <results.json>] [--digest-out <digest.txt>]
//!   conic-xtest compare <baseline.json> <candidate.json> [<candidate2.json> ...]

use fid_solv::conic::xtest::{compare_result_bytes, run_suite, DEFAULT_REL_TOL};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn usage() -> &'static str {
    "usage:\n  conic-xtest run --fixtures <dir> [--out <results.json>] [--digest-out <digest.txt>]\n  conic-xtest compare <baseline.json> <candidate.json> [<candidate2.json> ...]"
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run_cli(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            let _ = writeln!(io::stderr(), "{msg}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli(args: &[String]) -> Result<(), String> {
    let Some(cmd) = args.first().map(String::as_str) else {
        return Err(usage().to_string());
    };
    match cmd {
        "run" => cmd_run(&args[1..]),
        "compare" => cmd_compare(&args[1..]),
        "--help" | "-h" => {
            let _ = writeln!(io::stdout(), "{}", usage());
            Ok(())
        }
        other => Err(format!("unknown subcommand {other:?}\n{}", usage())),
    }
}

fn cmd_run(args: &[String]) -> Result<(), String> {
    let mut fixtures_dir: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut digest_out: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--fixtures" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    return Err("--fixtures requires a directory path".into());
                };
                fixtures_dir = Some(PathBuf::from(val));
            }
            "--out" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    return Err("--out requires a file path".into());
                };
                out_path = Some(PathBuf::from(val));
            }
            "--digest-out" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    return Err("--digest-out requires a file path".into());
                };
                digest_out = Some(PathBuf::from(val));
            }
            other => return Err(format!("unknown option {other:?}\n{}", usage())),
        }
        i += 1;
    }

    let dir = fixtures_dir.ok_or_else(|| format!("missing --fixtures <dir>\n{}", usage()))?;
    let manifest_path = dir.join("manifest.json");
    let manifest_bytes =
        fs::read(&manifest_path).map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let problems_path = dir.join("problems.json");
    let problems_bytes =
        fs::read(&problems_path).map_err(|e| format!("cannot read {}: {e}", problems_path.display()))?;

    let run = run_suite(&manifest_bytes, &problems_bytes, DEFAULT_REL_TOL).map_err(|e| format!("{e}"))?;

    if let Some(p) = &out_path {
        write_file(p, &run.canonical_json)?;
    } else {
        io::stdout().write_all(&run.canonical_json).map_err(|e| format!("stdout write failed: {e}"))?;
        let _ = writeln!(io::stdout());
    }

    let digest_line = format!("{}\n", run.digest);
    if let Some(dp) = &digest_out {
        write_file(dp, digest_line.as_bytes())?;
    }
    let _ = writeln!(
        io::stderr(),
        "conic-xtest: {} ({} problems: {} LP, {} QP, {} SOCP) digest {}",
        run.version,
        run.problem_count,
        run.lp_count,
        run.qp_count,
        run.socp_count,
        run.digest
    );
    Ok(())
}

fn cmd_compare(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err(format!("compare requires at least 2 result files\n{}", usage()));
    }
    let base_path = Path::new(&args[0]);
    let base_bytes = fs::read(base_path).map_err(|e| format!("cannot read {}: {e}", base_path.display()))?;
    let mut final_digest = None;

    for cand_arg in &args[1..] {
        let cand_path = Path::new(cand_arg);
        let cand_bytes = fs::read(cand_path).map_err(|e| format!("cannot read {}: {e}", cand_path.display()))?;
        match compare_result_bytes(&base_bytes, &cand_bytes) {
            Ok(digest) => final_digest = Some(digest),
            Err(err) => {
                return Err(format!("{} vs {}: {err}", base_path.display(), cand_path.display()));
            }
        }
    }

    if let Some(d) = final_digest {
        let _ = writeln!(io::stdout(), "ok: {} targets match ({d})", args.len());
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}
