//! `corpus-hostile`: CI tool of spec node IO.CORP.HOSTILE.
//!
//! ```text
//! corpus-hostile --corpus <dir> --plan <plan.toml> --out <dir> --seed <u64> [--max-total-bytes <n>]
//! ```
//!
//! Reads every corpus manifest in `--corpus`, applies the mutators of each format's plan to the
//! accepted in-repository files and writes the variants and one generated manifest per format
//! into `--out`; files there with the generated names are replaced and nothing else is touched.
//! Same seed and inputs give byte-identical output.
//! Exit status: 0 on success, 1 on a generation error, 2 on usage errors.

use fid_io_corp::{parse_plans, run_hostile, HostileOptions, DEFAULT_MAX_TOTAL_BYTES};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: corpus-hostile --corpus <dir> --plan <plan.toml> --out <dir> --seed <u64> [--max-total-bytes <n>]";
const MAX_PLAN_BYTES: u64 = 1024 * 1024;

struct Args {
    corpus: PathBuf,
    plan: PathBuf,
    out: PathBuf,
    seed: u64,
    max_total_bytes: u64,
}

fn parse_args() -> Result<Args, String> {
    let mut corpus = None;
    let mut plan = None;
    let mut out = None;
    let mut seed = None;
    let mut max_total_bytes = DEFAULT_MAX_TOTAL_BYTES;
    let mut it = std::env::args_os().skip(1);
    while let Some(flag) = it.next() {
        let flag = flag.to_string_lossy().into_owned();
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let int = |what: &str| {
            value.to_str().and_then(|v| v.parse::<u64>().ok()).ok_or_else(|| format!("{what} needs an integer"))
        };
        match flag.as_str() {
            "--corpus" => corpus = Some(PathBuf::from(&value)),
            "--plan" => plan = Some(PathBuf::from(&value)),
            "--out" => out = Some(PathBuf::from(&value)),
            "--seed" => seed = Some(int("--seed")?),
            "--max-total-bytes" => max_total_bytes = int("--max-total-bytes")?,
            _ => return Err(format!("unknown argument {flag:?}")),
        }
    }
    Ok(Args {
        corpus: corpus.ok_or("--corpus is required")?,
        plan: plan.ok_or("--plan is required")?,
        out: out.ok_or("--out is required")?,
        seed: seed.ok_or("--seed is required")?,
        max_total_bytes,
    })
}

fn run() -> Result<Result<String, String>, String> {
    let args = parse_args()?;
    let mut text = String::new();
    std::fs::File::open(&args.plan)
        .and_then(|f| f.take(MAX_PLAN_BYTES).read_to_string(&mut text))
        .map_err(|e| format!("cannot read {}: {e}", args.plan.display()))?;
    let plans = match parse_plans(&text) {
        Ok(p) => p,
        Err(e) => return Ok(Err(e.to_string())),
    };
    let opts = HostileOptions { max_total_bytes: args.max_total_bytes, ..HostileOptions::new(args.seed) };
    Ok(match run_hostile(&args.corpus, &plans, &args.out, &opts) {
        Ok(report) => {
            let n: usize = report.formats.values().map(Vec::len).sum();
            Ok(format!(
                "corpus-hostile: {} formats, {n} hostile variants, {} bytes written",
                report.formats.len(),
                report.bytes_written
            ))
        }
        Err(e) => Err(e.to_string()),
    })
}

fn main() -> ExitCode {
    match run() {
        Ok(Ok(summary)) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Ok(Err(e)) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("corpus-hostile: {e}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
