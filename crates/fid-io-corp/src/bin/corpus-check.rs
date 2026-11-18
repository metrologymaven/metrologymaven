//! `corpus-check`: CI tool of spec node IO.CORP.MANIF.
//!
//! ```text
//! corpus-check --corpus <dir> --formats <list-formats.json> [--cache <dir>] [--max-bytes <n>]
//! ```
//!
//! `--formats` is the JSON output of `fid import list-formats --json` (`import.list_formats@1`).
//! With `--cache`, `url` entries are fetched into that directory and verified; without it they are
//! skipped. Exit status: 0 when every check passes, 1 when any finding is reported, 2 on usage errors.

use fid_io_corp::{
    check_corpus, parse_format_listing, CheckOptions, Fetching, SystemFetcher, DEFAULT_MAX_BYTES, MAX_MANIFEST_BYTES,
};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: corpus-check --corpus <dir> --formats <list-formats.json> [--cache <dir>] [--max-bytes <n>]";

struct Args {
    corpus: PathBuf,
    formats: PathBuf,
    cache: Option<PathBuf>,
    max_bytes: u64,
}

fn parse_args() -> Result<Args, String> {
    let mut corpus = None;
    let mut formats = None;
    let mut cache = None;
    let mut max_bytes = DEFAULT_MAX_BYTES;
    let mut it = std::env::args_os().skip(1);
    while let Some(flag) = it.next() {
        let flag = flag.to_string_lossy().into_owned();
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--corpus" => corpus = Some(PathBuf::from(value)),
            "--formats" => formats = Some(PathBuf::from(value)),
            "--cache" => cache = Some(PathBuf::from(value)),
            "--max-bytes" => {
                max_bytes = value.to_str().and_then(|v| v.parse().ok()).ok_or("--max-bytes needs an integer")?
            }
            _ => return Err(format!("unknown argument {flag:?}")),
        }
    }
    Ok(Args {
        corpus: corpus.ok_or("--corpus is required")?,
        formats: formats.ok_or("--formats is required")?,
        cache,
        max_bytes,
    })
}

fn run() -> Result<bool, String> {
    let args = parse_args()?;
    let mut listing = Vec::new();
    std::fs::File::open(&args.formats)
        .and_then(|f| f.take(MAX_MANIFEST_BYTES * 16).read_to_end(&mut listing))
        .map_err(|e| format!("cannot read {}: {e}", args.formats.display()))?;
    let formats = parse_format_listing(&listing).map_err(|e| e.to_string())?;
    let fetcher = SystemFetcher { max_bytes: args.max_bytes };
    let fetch = args.cache.as_deref().map(|cache| Fetching { cache, fetcher: &fetcher });
    let report = check_corpus(&args.corpus, &formats, &CheckOptions { max_bytes: args.max_bytes, fetch });
    for e in &report.errors {
        eprintln!("error: {e}");
    }
    println!(
        "corpus-check: {} formats registered, {} manifests, {} files checked, {} fetched, {} cached, {} urls skipped, {} errors",
        formats.len(),
        report.manifests.len(),
        report.files_checked,
        report.fetched,
        report.cached,
        report.urls_skipped,
        report.errors.len()
    );
    Ok(report.is_ok())
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("corpus-check: {e}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
