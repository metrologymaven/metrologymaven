//! `fid-eval-redteam`: the `fid-eval adversary` words (D-032; outside the registry, never on
//! MCP). Spec node AGENT.REDTEAM.SCRIPT.

use fid_eval_redteam::{
    read_manifest, read_task, run, run_with_server, session::line, AdversaryError, ProcessCli, TaskView,
};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
fid-eval-redteam: Metrology Maven scripted red-team adversary (pre-alpha); the `fid-eval adversary` words

USAGE:
    fid-eval-redteam adversary --task <FILE> --manifest <FILE> [--mcp-config <FILE>] [--fid <PROGRAM>]
                               Attempt every forbidden action of the task by every route over MCP.
                               Without --mcp-config, speak MCP on this process's stdin/stdout;
                               with it, start the one server it names and speak on its stdio.
                               Shell tasks also call <PROGRAM> (default `fid`) for each attempt.
                               The plan and one outcome line per attempt go to stderr.
    fid-eval-redteam adversary --task <FILE> --manifest <FILE> --plan
                               Print the attempt plan (canonical JSON lines) and exit.
    fid-eval-redteam --version | --help

No LLM, no clock, no randomness: for a given task, manifest and server the request stream is
byte-identical across runs. Run it only inside a harness sandbox.
Exit codes: 0 done (refusals are expected), 1 invalid input or I/O failure, 2 usage error.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["--version" | "-V"] => {
            println!("fid-eval-redteam {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [] | ["--help" | "-h" | "help"] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["adversary", rest @ ..] => adversary(rest),
        _ => usage_error("unknown command"),
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

#[derive(Default)]
struct Opts<'a> {
    task: Option<&'a str>,
    manifest: Option<&'a str>,
    mcp_config: Option<&'a str>,
    fid: Option<&'a str>,
    plan: bool,
}

fn adversary(args: &[&str]) -> ExitCode {
    let mut o = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let slot = match args[i] {
            "--plan" => {
                o.plan = true;
                i += 1;
                continue;
            }
            "--task" => &mut o.task,
            "--manifest" => &mut o.manifest,
            "--mcp-config" => &mut o.mcp_config,
            "--fid" => &mut o.fid,
            other => return usage_error(&format!("unknown argument {other:?}")),
        };
        let Some(v) = args.get(i + 1) else {
            return usage_error(&format!("{} needs a value", args[i]));
        };
        if slot.replace(*v).is_some() {
            return usage_error(&format!("{} given twice", args[i]));
        }
        i += 2;
    }
    let (Some(task), Some(manifest)) = (o.task, o.manifest) else {
        return usage_error("--task and --manifest are required");
    };
    match go(task, manifest, &o) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn go(task: &str, manifest: &str, o: &Opts) -> Result<(), AdversaryError> {
    let manifest = read_manifest(Path::new(manifest))?;
    let task: TaskView = read_task(Path::new(task))?;
    let mut err = std::io::stderr().lock();
    if o.plan {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        for a in fid_eval_redteam::build_plan(&task, &manifest) {
            out.write_all(&line(&a.to_value())?).map_err(|e| AdversaryError::Io(e.to_string()))?;
        }
        return out.flush().map_err(|e| AdversaryError::Io(e.to_string()));
    }
    let mut cli = ProcessCli { program: o.fid.unwrap_or("fid").to_string() };
    match o.mcp_config {
        Some(cfg) => {
            let bytes = std::fs::read(cfg).map_err(|e| AdversaryError::InvalidMcpConfig(format!("{cfg}: {e}")))?;
            let launch = fid_eval_redteam::parse_mcp_config(&bytes)?;
            run_with_server(&task, &manifest, &launch, &mut err, &mut cli)?;
        }
        None => {
            let stdin = std::io::stdin().lock();
            let stdout = std::io::stdout().lock();
            run(&task, &manifest, stdin, stdout, &mut err, &mut cli)?;
        }
    }
    Ok(())
}
