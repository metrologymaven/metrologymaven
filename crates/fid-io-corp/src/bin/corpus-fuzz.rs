//! `corpus-fuzz`: CI tool of spec node `IO.CORP.FUZZ`.
//!
//! ```text
//! corpus-fuzz check    --formats <list-formats.json> --targets <fuzz_targets_dir>
//! corpus-fuzz generate --formats <list-formats.json> --out <fuzz_dir> [--seeds <seeds_dir>]
//! corpus-fuzz seed     --corpus <dir> --formats <list-formats.json> --out <seeds_dir> [--cache <dir>]
//! corpus-fuzz run      --formats <list-formats.json> --targets <fuzz_targets_dir>
//!                      --seeds <seeds_dir> --artifacts <artifacts_dir>
//!                      [--budget-secs <N>] [--timeout-secs <N>] [--max-iters <N>] [--seed <N>]
//! ```
//!
//! Exit status: 0 ok, 1 coverage gap (`fuzz_target_missing`) or fuzz finding (panic/timeout),
//! 2 usage or I/O error.

use fid_io_corp::{
    builtin_readers, check_coverage, generate_targets, load_manifests, parse_format_listing, run_fuzz_suite,
    stage_seeds, FuzzConfig, FUZZ_MERGE_BUDGET_SECS, FUZZ_PER_INPUT_TIMEOUT_SECS, MAX_MANIFEST_BYTES,
};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
usage:
  corpus-fuzz check    --formats <list-formats.json> --targets <fuzz_targets_dir>
  corpus-fuzz generate --formats <list-formats.json> --out <fuzz_dir> [--seeds <seeds_dir>]
  corpus-fuzz seed     --corpus <dir> --formats <list-formats.json> --out <seeds_dir> [--cache <dir>]
  corpus-fuzz run      --formats <list-formats.json> --targets <fuzz_targets_dir> --seeds <seeds_dir> --artifacts <artifacts_dir> [--budget-secs <N>] [--timeout-secs <N>] [--max-iters <N>] [--seed <N>]";

fn read_formats(path: &Path) -> Result<Vec<fid_io_corp::FormatId>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_MANIFEST_BYTES * 16).read_to_end(&mut bytes))
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse_format_listing(&bytes).map_err(|e| e.to_string())
}

fn req_flag(map: &std::collections::BTreeMap<String, String>, key: &str) -> Result<String, String> {
    map.get(key).cloned().ok_or_else(|| format!("{key} is required"))
}

fn parse_flags(args: &[String]) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut i = 0usize;
    while i < args.len() {
        let Some(flag) = args.get(i) else {
            break;
        };
        if !flag.starts_with("--") {
            return Err(format!("unexpected positional argument {flag:?}"));
        }
        let Some(val) = args.get(i + 1) else {
            return Err(format!("{flag} needs a value"));
        };
        if out.insert(flag.clone(), val.clone()).is_some() {
            return Err(format!("duplicate flag {flag}"));
        }
        i += 2;
    }
    Ok(out)
}

fn cmd_check(args: &[String]) -> Result<bool, String> {
    let flags = parse_flags(args)?;
    let formats = read_formats(Path::new(&req_flag(&flags, "--formats")?))?;
    let targets_dir = PathBuf::from(req_flag(&flags, "--targets")?);
    let report = check_coverage(&formats, &targets_dir);
    for err in &report.missing {
        eprintln!("error: {err}");
    }
    println!(
        "corpus-fuzz check: {} formats registered, {} covered, {} missing",
        report.checked,
        report.present.len(),
        report.missing.len()
    );
    Ok(report.is_ok())
}

fn cmd_generate(args: &[String]) -> Result<bool, String> {
    let flags = parse_flags(args)?;
    let formats = read_formats(Path::new(&req_flag(&flags, "--formats")?))?;
    let out_dir = PathBuf::from(req_flag(&flags, "--out")?);
    let seeds_root = flags.get("--seeds").map_or_else(|| out_dir.join("corpus"), PathBuf::from);
    let targets = generate_targets(&formats, &out_dir, &seeds_root).map_err(|e| e.to_string())?;
    println!("corpus-fuzz generate: wrote {} targets under {}", targets.len(), out_dir.display());
    Ok(true)
}

fn cmd_seed(args: &[String]) -> Result<bool, String> {
    let flags = parse_flags(args)?;
    let corpus_dir = PathBuf::from(req_flag(&flags, "--corpus")?);
    let formats = read_formats(Path::new(&req_flag(&flags, "--formats")?))?;
    let out_dir = PathBuf::from(req_flag(&flags, "--out")?);
    let cache = flags.get("--cache").map(PathBuf::from);
    let (all_manifests, manifest_errors) = load_manifests(&corpus_dir);
    for e in &manifest_errors {
        eprintln!("error: {e}");
    }
    let selected: Vec<_> = formats.iter().filter_map(|f| all_manifests.get(f).cloned()).collect();
    let report = stage_seeds(&corpus_dir, &selected, cache.as_deref(), &out_dir);
    for e in &report.errors {
        eprintln!("error: {e}");
    }
    println!(
        "corpus-fuzz seed: {} targets, {} seeds staged, {} urls skipped, {} errors",
        report.targets,
        report.seeds_staged,
        report.urls_skipped,
        manifest_errors.len() + report.errors.len()
    );
    Ok(manifest_errors.is_empty() && report.is_ok())
}

fn cmd_run(args: &[String]) -> Result<bool, String> {
    let flags = parse_flags(args)?;
    let formats = read_formats(Path::new(&req_flag(&flags, "--formats")?))?;
    let targets_dir = PathBuf::from(req_flag(&flags, "--targets")?);
    let seeds_root = PathBuf::from(req_flag(&flags, "--seeds")?);
    let artifacts_dir = PathBuf::from(req_flag(&flags, "--artifacts")?);

    let budget_secs: u64 = match flags.get("--budget-secs") {
        Some(v) => v.parse().map_err(|_| "--budget-secs must be a non-negative integer".to_string())?,
        None => FUZZ_MERGE_BUDGET_SECS,
    };
    let timeout_secs: u64 = match flags.get("--timeout-secs") {
        Some(v) => v.parse().map_err(|_| "--timeout-secs must be a positive integer".to_string())?,
        None => FUZZ_PER_INPUT_TIMEOUT_SECS,
    };
    if timeout_secs == 0 {
        return Err("--timeout-secs must be positive".to_string());
    }
    let max_iterations: Option<u64> = match flags.get("--max-iters") {
        Some(v) => Some(v.parse().map_err(|_| "--max-iters must be an integer".to_string())?),
        None => None,
    };
    let seed: u64 = match flags.get("--seed") {
        Some(v) => v.parse().map_err(|_| "--seed must be an integer".to_string())?,
        None => FuzzConfig::default().seed,
    };

    let config = FuzzConfig {
        budget: Duration::from_secs(budget_secs),
        per_input_timeout: Duration::from_secs(timeout_secs),
        max_iterations,
        seed,
        minimize: true,
    };
    let readers = builtin_readers();
    let report = run_fuzz_suite(&formats, &readers, &seeds_root, &artifacts_dir, &targets_dir, &config)
        .map_err(|e| e.to_string())?;
    for err in &report.coverage_errors {
        eprintln!("error: {err}");
    }
    for outcome in &report.targets {
        if let Some(f) = &outcome.finding {
            eprintln!(
                "finding: {} [{}] ({} -> {} bytes, sha256 {}) saved at {}: {}",
                f.reader_id,
                f.kind,
                f.original_len,
                f.minimised_len,
                f.minimised_sha256,
                f.artifact_path.display(),
                f.detail
            );
        } else {
            println!("ok: {} ({} executions)", outcome.reader_id, outcome.executions);
        }
    }
    Ok(report.is_ok())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(sub) = args.first().map(String::as_str) else {
        eprintln!("corpus-fuzz: missing subcommand\n{USAGE}");
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    let res = match sub {
        "--help" | "-h" | "help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        "check" => cmd_check(rest),
        "generate" => cmd_generate(rest),
        "seed" => cmd_seed(rest),
        "run" => cmd_run(rest),
        other => Err(format!("unknown subcommand {other:?}")),
    };
    match res {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("corpus-fuzz: {e}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
