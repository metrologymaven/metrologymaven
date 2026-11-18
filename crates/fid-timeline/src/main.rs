//! `fid-timeline --events <json> --now <RFC3339> [--config <toml-or-json>] [--json]`
//! (spec node `ECO.COMM.TIMELINE`).
//!
//! Prints the action due and the author-blocked clock and exits 0. On malformed input
//! it prints `TIMELINE-INPUT: ...` or `TIMELINE-CONFIG: ...` to stderr (and, with
//! `--json`, `{"error":"<code>"}` to stdout, with no action) and exits 1. Bad usage exits 2.
//! Reads only the named files: no network, no clock, no environment.

use fid_timeline::{evaluate_inputs, TimelineError, MAX_CONFIG_BYTES, MAX_INPUT_BYTES};
use std::io::Read;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-timeline --events <json> --now <YYYY-MM-DDTHH:MM:SSZ> [--config <toml-or-json>] [--json]";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

/// Reads at most `limit + 1` bytes, so oversized files fail the size check without
/// being read whole.
fn read_bounded(path: &str, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    std::fs::File::open(path)?.take(cap).read_to_end(&mut buf)?;
    Ok(buf)
}

fn run(events: &str, now: &str, config: Option<&str>) -> Result<fid_timeline::Evaluation, TimelineError> {
    let ev = read_bounded(events, MAX_INPUT_BYTES)
        .map_err(|e| TimelineError::input(format!("cannot read event file {events:?}: {e}")))?;
    let cfg = match config {
        Some(p) => {
            let bytes = read_bounded(p, MAX_CONFIG_BYTES)
                .map_err(|e| TimelineError::config(format!("cannot read config {p:?}: {e}")))?;
            Some(String::from_utf8(bytes).map_err(|_| TimelineError::config("config is not UTF-8"))?)
        }
        None => None,
    };
    evaluate_inputs(&ev, cfg.as_deref(), now)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut events: Option<String> = None;
    let mut now: Option<String> = None;
    let mut config: Option<String> = None;
    let mut json = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--events" | "--now" | "--config" => {
                let Some(v) = it.next() else {
                    return usage();
                };
                match a.as_str() {
                    "--events" => events = Some(v),
                    "--now" => now = Some(v),
                    _ => config = Some(v),
                }
            }
            _ => return usage(),
        }
    }
    let (Some(events), Some(now)) = (events, now) else {
        return usage();
    };
    match run(&events, &now, config.as_deref()) {
        Ok(ev) => {
            if json {
                println!("{}", ev.to_json());
            } else {
                println!("action: {}", ev.action.as_str());
                println!("clock_days: {}", ev.clock_days);
                println!("clock_seconds: {}", ev.clock_seconds);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            if json {
                println!("{{\"error\":\"{}\"}}", e.code.as_str());
            }
            ExitCode::from(1)
        }
    }
}
