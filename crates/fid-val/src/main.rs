//! CLI entry point for `fid-val` (`KRN.MAN.VAL.RUN`).

use fid_val::{load_criteria_dir, load_mapping_table, load_release, run_validation, CommandArtifactRunner};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

fn usage() {
    eprintln!(
        "usage: fid-val run --release <release.json> --mappings <mappings.toml> \
         --criteria <criteria-dir> --artifact <program> [--arg <arg> ...] [--out <file>]"
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        usage();
        return ExitCode::from(2);
    };
    if cmd != "run" {
        usage();
        return ExitCode::from(2);
    }

    let mut release_path: Option<PathBuf> = None;
    let mut mappings_path: Option<PathBuf> = None;
    let mut criteria_dir: Option<PathBuf> = None;
    let mut artifact_prog: Option<String> = None;
    let mut artifact_args: Vec<String> = Vec::new();
    let mut out_path: Option<PathBuf> = None;
    let mut timeout_ms: u64 = 30_000;

    let mut i = 0usize;
    while let Some(flag) = rest.get(i) {
        let next = rest.get(i + 1);
        match flag.as_str() {
            "--release" => {
                let Some(v) = next else {
                    eprintln!("missing value for --release");
                    return ExitCode::from(2);
                };
                release_path = Some(PathBuf::from(v));
                i += 2;
            }
            "--mappings" => {
                let Some(v) = next else {
                    eprintln!("missing value for --mappings");
                    return ExitCode::from(2);
                };
                mappings_path = Some(PathBuf::from(v));
                i += 2;
            }
            "--criteria" => {
                let Some(v) = next else {
                    eprintln!("missing value for --criteria");
                    return ExitCode::from(2);
                };
                criteria_dir = Some(PathBuf::from(v));
                i += 2;
            }
            "--artifact" => {
                let Some(v) = next else {
                    eprintln!("missing value for --artifact");
                    return ExitCode::from(2);
                };
                artifact_prog = Some(v.clone());
                i += 2;
            }
            "--arg" => {
                let Some(v) = next else {
                    eprintln!("missing value for --arg");
                    return ExitCode::from(2);
                };
                artifact_args.push(v.clone());
                i += 2;
            }
            "--out" => {
                let Some(v) = next else {
                    eprintln!("missing value for --out");
                    return ExitCode::from(2);
                };
                out_path = Some(PathBuf::from(v));
                i += 2;
            }
            "--timeout-ms" => {
                let Some(v) = next else {
                    eprintln!("missing value for --timeout-ms");
                    return ExitCode::from(2);
                };
                let Ok(ms) = v.parse::<u64>() else {
                    eprintln!("invalid --timeout-ms: {v}");
                    return ExitCode::from(2);
                };
                timeout_ms = ms;
                i += 2;
            }
            other => {
                eprintln!("unknown option: {other}");
                usage();
                return ExitCode::from(2);
            }
        }
    }

    let (Some(rel_p), Some(map_p), Some(crit_p), Some(prog)) =
        (release_path, mappings_path, criteria_dir, artifact_prog)
    else {
        usage();
        return ExitCode::from(2);
    };

    let release = match load_release(&rel_p) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let mappings = match load_mapping_table(&map_p) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let criteria = match load_criteria_dir(&crit_p) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    let mut runner =
        CommandArtifactRunner { program: prog, args: artifact_args, timeout: Duration::from_millis(timeout_ms) };
    let results = match run_validation(&release, &mappings, &criteria, &mut runner) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let bytes = match results.to_canonical_json() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    if let Some(out) = out_path {
        if let Err(e) = std::fs::write(&out, &bytes) {
            eprintln!("cannot write {}: {e}", out.display());
            return ExitCode::from(1);
        }
    } else if let Err(e) = std::io::Write::write_all(&mut std::io::stdout(), &bytes) {
        eprintln!("stdout write failed: {e}");
        return ExitCode::from(1);
    }

    let any_failed = results.summaries.iter().any(|s| s.failed > 0);
    if any_failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
