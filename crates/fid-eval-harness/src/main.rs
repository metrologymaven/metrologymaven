//! `fid-eval-harness`: the `fid-eval adapter ...` and `fid-eval safety` words of the evaluation
//! harness (D-032; outside the registry, never on MCP), until the `fid-eval` dispatcher forwards
//! them.

use fid_canon::CValue;
use fid_eval_harness::{suite_dir_of, Adapter, AdapterError, AgentPaths, AgentTask, ScoreOptions};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
fid-eval-harness: Metrology Maven evaluation harness (pre-alpha)

USAGE:
    fid-eval-harness adapter check <adapter.json> [--task <DIR>]
                                  Check an adapter file strictly (unknown fields,
                                  unknown or missing placeholders, empty argv, oracle
                                  or task-file paths, undeclared shell, absolute paths
                                  into the suite folder of <DIR>). With --task, also
                                  print the argv resolved for that task, with the
                                  nominal paths <run>/agent/prompt.txt,
                                  <run>/agent/mcp-config.json and <run>/project
    fid-eval-harness safety <run-dir> [--json]
                                  Count refused attempts and committed violations of
                                  a finished run by category (forbidden_class,
                                  self_adoption, out_of_root, raw_path, redteam) from
                                  <run-dir>/artifacts/; --json prints the report as
                                  one line of canonical JSON
    fid-eval-harness score <run-dir> [--json]
                                  Score a finished or archived run against its oracle,
                                  combining safety violations and show-brief verdict
                                  into computed adoptability; writes artifacts/score.json
                                  and prints text or one line of canonical JSON
    fid-eval-harness null-agent [ARGS...]
                                  Exit 0 at once without reading anything or making
                                  a tool call (the `null` reference adapter)
    fid-eval-harness --version    Print the version
    fid-eval-harness --help       Print this help

Exit codes: 0 ok (for safety/score: the report was produced, whatever it counts), 1 invalid
adapter, task or run artifacts, 2 usage error.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["null-agent", ..] => ExitCode::SUCCESS,
        ["--version" | "-V"] => {
            println!("fid-eval-harness {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [] | ["--help" | "-h" | "help"] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["safety", dir] => safety(dir, false),
        ["safety", dir, "--json"] | ["safety", "--json", dir] => safety(dir, true),
        ["score", dir] => score_cmd(dir, false),
        ["score", dir, "--json"] | ["score", "--json", dir] => score_cmd(dir, true),
        ["adapter", "check", file] => check(file, None),
        ["adapter", "check", file, "--task", task] | ["adapter", "check", "--task", task, file] => {
            check(file, Some(*task))
        }
        _ => {
            eprintln!("error: unknown command\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn score_cmd(dir: &str, json: bool) -> ExitCode {
    let opts = ScoreOptions { write_artifacts: true };
    let score = match fid_eval_harness::score_run(Path::new(dir), &opts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    if !json {
        print!("{}", score.to_text());
        return ExitCode::SUCCESS;
    }
    match score.to_json() {
        Ok(b) => {
            println!("{}", String::from_utf8_lossy(&b));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn safety(dir: &str, json: bool) -> ExitCode {
    let report = match fid_eval_harness::safety_report(Path::new(dir)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    if !json {
        print!("{}", report.to_text());
        return ExitCode::SUCCESS;
    }
    match fid_canon::to_canonical_bytes(&report.to_cvalue()) {
        Ok(b) => {
            println!("{}", String::from_utf8_lossy(&b));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn fail(e: &AdapterError) -> ExitCode {
    for p in &e.problems {
        eprintln!("{p}");
    }
    eprintln!("error: {} ({} problem(s))", e.code(), e.problems.len());
    ExitCode::from(1)
}

fn check(file: &str, task_dir: Option<&str>) -> ExitCode {
    let task = match task_dir.map(|d| AgentTask::read(Path::new(d))).transpose() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let suite = task_dir.and_then(|d| suite_dir_of(Path::new(d)));
    let adapter = match Adapter::read(Path::new(file), suite.as_deref()) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    println!(
        "ok: adapter {} {} (prompt {}, shell {})",
        adapter.name,
        adapter.version,
        adapter.prompt.as_str(),
        adapter.shell
    );
    let Some(task) = task else {
        return ExitCode::SUCCESS;
    };
    let Some(suite) = suite else {
        eprintln!("invalid_task: the task folder is not <suite>/tasks/<id>");
        return ExitCode::from(1);
    };
    if let Err(e) = fid_eval_harness::check_profile(&task.profile) {
        eprintln!("{e}");
        return ExitCode::from(1);
    }
    let paths = AgentPaths {
        prompt_file: "<run>/agent/prompt.txt".to_string(),
        mcp_config: "<run>/agent/mcp-config.json".to_string(),
        workdir: "<run>/project".to_string(),
    };
    match adapter.resolve_argv(&paths, &task.prompt, &suite) {
        Ok(argv) => {
            let v = CValue::Arr(argv.into_iter().map(CValue::Str).collect());
            match fid_canon::to_canonical_bytes(&v) {
                Ok(b) => {
                    println!("task: {}", task.id);
                    println!("argv: {}", String::from_utf8_lossy(&b));
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Err(e) => fail(&e),
    }
}
