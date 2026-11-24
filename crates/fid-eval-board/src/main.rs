//! `fid-eval-board`: the `fid-eval board` subcommands (D-032; outside the registry, never on MCP).
//!
//! First slice: `board check-claims` (spec node AGENT.BOARD.CLAIMS).

use fid_canon::CValue;
use fid_eval_board::{check_claims, write_guide, Finding, GuideInputs, Inputs, Report};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
fid-eval-board: Metrology Maven scoreboard tooling (pre-alpha); the `fid-eval board` and `fid-eval local` words

USAGE:
    fid-eval-board board check-claims --register <FILE> --board <BUNDLE> --sources <PATH>...
                                      [--phrases <FILE>] [--json]
                                  Check public capability claims: every `<!-- fid-claim: <id> -->`
                                  tag in the Markdown/HTML sources names a register entry, every
                                  entry's board figure meets its threshold (exact fractions) on a
                                  release whose red-team verdict is `pass`, and no paragraph uses
                                  a capability phrase without a tag. <BUNDLE> is a bundle folder
                                  holding board.json, or the file itself. With one board,
                                  entries for other (e.g. earlier) releases report
                                  entry_missing; this is intended. The phrase list
                                  defaults to claim-phrases.txt next to the register. With
                                  --json print {\"claims\",\"files\",\"findings\",\"ok\",\"tags\"}.
    fid-eval-board local guide --board <BUNDLE> --catalog <CATALOG_INDEX> --hosts <DIR> --out <DIR>
                                  Generate static Markdown and HTML open-weight model guidance
                                  (local-model-guide.md and local-model-guide.html) from board.json,
                                  the curated catalog index, and host.json/model.json metadata.
    fid-eval-board --version      Print the version
    fid-eval-board --help         Print this help

Reads local files only. Exit codes: 0 pass, 1 findings or invalid input, 2 usage error.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["--version" | "-V"] => {
            println!("fid-eval-board {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [] | ["--help" | "-h" | "help"] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["board", "check-claims", rest @ ..] | ["check-claims", rest @ ..] => check(rest),
        ["local", "guide", rest @ ..] | ["guide", rest @ ..] => run_guide(rest),
        _ => usage_error("unknown command"),
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

fn run_guide(args: &[&str]) -> ExitCode {
    let mut board: Option<&str> = None;
    let mut catalog: Option<&str> = None;
    let mut hosts: Option<&str> = None;
    let mut out: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        i += 1;
        let slot = match a {
            "--board" => &mut board,
            "--catalog" => &mut catalog,
            "--hosts" => &mut hosts,
            "--out" => &mut out,
            other => return usage_error(&format!("unknown argument `{other}`")),
        };
        match args.get(i) {
            Some(v) if !v.starts_with("--") && slot.is_none() => {
                *slot = Some(*v);
                i += 1;
            }
            Some(v) if !v.starts_with("--") => return usage_error(&format!("{a} given twice")),
            _ => return usage_error(&format!("{a} needs a value")),
        }
    }
    let (Some(board), Some(catalog), Some(hosts), Some(out)) = (board, catalog, hosts, out) else {
        return usage_error("--board <BUNDLE>, --catalog <FILE>, --hosts <DIR> and --out <DIR> are required");
    };
    let inputs = GuideInputs { board: Path::new(board), catalog: Path::new(catalog), hosts: Path::new(hosts) };
    match write_guide(&inputs, Path::new(out)) {
        Ok(guide) => {
            println!(
                "ok: generated guide for {} ({} open-weight model(s), {} workflow(s))",
                guide.release,
                guide.models.len(),
                guide.workflows.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn check(args: &[&str]) -> ExitCode {
    let mut register: Option<&str> = None;
    let mut board: Option<&str> = None;
    let mut phrases: Option<&str> = None;
    let mut sources: Vec<PathBuf> = Vec::new();
    let mut json = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        i += 1;
        let slot = match a {
            "--json" => {
                json = true;
                continue;
            }
            "--sources" => {
                let start = i;
                while i < args.len() && !args[i].starts_with("--") {
                    sources.push(PathBuf::from(args[i]));
                    i += 1;
                }
                if i == start {
                    return usage_error("--sources needs at least one path");
                }
                continue;
            }
            "--register" => &mut register,
            "--board" => &mut board,
            "--phrases" => &mut phrases,
            other => return usage_error(&format!("unknown argument `{other}`")),
        };
        match args.get(i) {
            Some(v) if !v.starts_with("--") && slot.is_none() => {
                *slot = Some(*v);
                i += 1;
            }
            Some(v) if !v.starts_with("--") => return usage_error(&format!("{a} given twice")),
            _ => return usage_error(&format!("{a} needs a value")),
        }
    }
    let (Some(register), Some(board)) = (register, board) else {
        return usage_error("--register <FILE> and --board <BUNDLE> are required");
    };
    if sources.is_empty() {
        return usage_error("--sources <PATH>... is required");
    }
    let inputs = Inputs {
        register: Path::new(register),
        board: Path::new(board),
        phrases: phrases.map(Path::new),
        sources: &sources,
    };
    let report = check_claims(&inputs);
    if json {
        print_json(&report);
    } else if report.ok() {
        println!("ok: {} claims, {} tags in {} files", report.claims, report.tags, report.files);
    } else {
        for f in &report.findings {
            eprintln!("{f}");
        }
        eprintln!("check-claims failed: {} finding(s)", report.findings.len());
    }
    if report.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn finding_json(f: &Finding) -> CValue {
    CValue::Obj(BTreeMap::from([
        ("claim".to_string(), f.claim.as_deref().map_or(CValue::Null, s)),
        ("code".to_string(), s(f.code.as_str())),
        ("detail".to_string(), s(&f.detail)),
        ("file".to_string(), s(&f.file)),
        ("line".to_string(), CValue::Num(f.line as f64)),
    ]))
}

fn print_json(r: &Report) {
    let v = CValue::Obj(BTreeMap::from([
        ("claims".to_string(), CValue::Num(r.claims as f64)),
        ("files".to_string(), CValue::Num(r.files as f64)),
        ("findings".to_string(), CValue::Arr(r.findings.iter().map(finding_json).collect())),
        ("ok".to_string(), CValue::Bool(r.ok())),
        ("tags".to_string(), CValue::Num(r.tags as f64)),
    ]));
    match fid_canon::to_canonical_bytes(&v) {
        Ok(b) => println!("{}", String::from_utf8_lossy(&b)),
        Err(e) => eprintln!("error: {e}"),
    }
}
