#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of spec node AGENT.HARNESS.ADAPT. Expected argv, error codes, the golden
//! MCP configuration and token-report verdicts are hand-labelled in fixtures/agent/harness/
//! (adapters/expected.json, mcp-config.golden.json, tokens/expected.json); nothing expected is
//! computed by this crate. No test starts a real agent or contacts any service: the only
//! programs launched are this crate's own binary (`adapter check` and `null-agent`).

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_eval_harness::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_fid-eval-harness");

/// A path below the repository root, spelled without `..` (which adapters may not contain).
fn root(rel: &str) -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().join(rel)
}

fn fixture(rel: &str) -> PathBuf {
    root("fixtures/agent/harness").join(rel)
}

fn task_dir() -> PathBuf {
    root("fixtures/agent/eval/suite-mini/tasks/mini-inspect-01")
}

fn suite_dir() -> PathBuf {
    root("fixtures/agent/eval/suite-mini")
}

fn load(p: &Path) -> CValue {
    parse(&std::fs::read(p).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("expected object, got {other:?}"),
    }
}

fn strs(v: &CValue) -> Vec<String> {
    match v {
        CValue::Arr(a) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                other => panic!("expected string, got {other:?}"),
            })
            .collect(),
        other => panic!("expected array, got {other:?}"),
    }
}

fn adapter_json(argv: &[&str], prompt: &str) -> Vec<u8> {
    let mut m = BTreeMap::new();
    m.insert("name".to_string(), CValue::Str("t".to_string()));
    m.insert("version".to_string(), CValue::Str("1".to_string()));
    m.insert("argv".to_string(), CValue::Arr(argv.iter().map(|s| CValue::Str(s.to_string())).collect()));
    m.insert("prompt".to_string(), CValue::Str(prompt.to_string()));
    m.insert("model".to_string(), CValue::Str("m".to_string()));
    m.insert("settings".to_string(), CValue::Obj(BTreeMap::new()));
    m.insert("shell".to_string(), CValue::Bool(false));
    to_canonical_bytes(&CValue::Obj(m)).unwrap()
}

fn paths() -> AgentPaths {
    AgentPaths {
        prompt_file: "/run/agent/prompt.txt".to_string(),
        mcp_config: "/run/agent/mcp-config.json".to_string(),
        workdir: "/run/project".to_string(),
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-adapt-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn codes(e: &AdapterError) -> Vec<String> {
    let mut c: Vec<String> = e.problems.iter().map(|p| p.code.as_str().to_string()).collect();
    c.sort();
    c
}

#[test]
fn ac1_exec_example_passes_and_prints_resolved_argv() {
    let exp = load(&fixture("adapters/expected.json"));
    let check = obj(obj(&exp).get("check").unwrap());
    let want = to_canonical_bytes(check.get("argv").unwrap()).unwrap();
    let out = Command::new(BIN)
        .args(["adapter", "check"])
        .arg(fixture("adapters/exec-example.json"))
        .arg("--task")
        .arg(task_dir())
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "stdout {stdout} stderr {}", String::from_utf8_lossy(&out.stderr));
    let line = format!("argv: {}", String::from_utf8(want).unwrap());
    assert!(stdout.lines().any(|l| l == line), "missing `{line}` in:\n{stdout}");
    assert!(stdout.starts_with("ok: adapter exec-example 1"), "{stdout}");
}

#[test]
fn reference_adapters_pass_and_match_the_fixture() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let exec = Adapter::read(&crate_dir.join("adapters/exec-example.json"), Some(&suite_dir())).unwrap();
    let fixture_exec = Adapter::read(&fixture("adapters/exec-example.json"), Some(&suite_dir())).unwrap();
    assert_eq!(exec, fixture_exec);
    assert_eq!(exec.prompt, PromptMode::File);
    assert_eq!(exec.token_report.as_deref(), Some("tokens.json"));
    assert!(!exec.shell);
    let null = Adapter::read(&crate_dir.join("adapters/null.json"), Some(&suite_dir())).unwrap();
    assert_eq!(null.argv.get(1).map(String::as_str), Some("null-agent"));
    assert!(!null.shell);
}

#[test]
fn ac2_mcp_config_matches_golden_except_sandbox_path() {
    let task = AgentTask::read(&task_dir()).unwrap();
    let project = tmp("ac2").join("project");
    let got = mcp_config(&task.profile, &project, &McpCommand::default()).unwrap();
    let golden = std::fs::read_to_string(fixture("mcp-config.golden.json")).unwrap();
    let sandbox = to_canonical_bytes(&CValue::Str(project.to_str().unwrap().to_string())).unwrap();
    let golden = golden.replace("\"<sandbox>\"", std::str::from_utf8(&sandbox).unwrap());
    assert_eq!(parse(&got).unwrap(), parse(golden.as_bytes()).unwrap());
    // Canonical bytes: same input, same bytes.
    assert_eq!(got, mcp_config(&task.profile, &project, &McpCommand::default()).unwrap());
    // Exactly one server.
    let top = parse(&got).unwrap();
    assert_eq!(obj(obj(&top).get("mcpServers").unwrap()).len(), 1);
}

#[test]
fn mcp_config_refuses_unsafe_profile_and_relative_project() {
    let project = tmp("mcp-bad").join("project");
    for profile in ["", "-x", "a b", "a;b", "../x"] {
        assert!(mcp_config(profile, &project, &McpCommand::default()).is_err(), "{profile:?}");
    }
    assert!(mcp_config("inspect-r1", Path::new("relative/project"), &McpCommand::default()).is_err());
}

#[test]
fn ac3_oracle_placeholder_fails_invalid_adapter() {
    let e = Adapter::read(&fixture("adapters/bad/oracle-placeholder.json"), None).unwrap_err();
    assert_eq!(e.code(), ErrorCode::InvalidAdapter);
    assert!(e.to_string().contains("{oracle}"), "{e}");
    let out = Command::new(BIN)
        .args(["adapter", "check"])
        .arg(fixture("adapters/bad/oracle-placeholder.json"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid_adapter"));
}

#[test]
fn hand_labelled_bad_adapters_report_each_problem() {
    let exp = load(&fixture("adapters/expected.json"));
    let bad = obj(obj(&exp).get("bad").unwrap());
    let mut seen = 0;
    for entry in std::fs::read_dir(fixture("adapters/bad")).unwrap() {
        let p = entry.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        let mut want = strs(bad.get(&name).unwrap_or_else(|| panic!("{name} is not labelled")));
        want.sort();
        let e = Adapter::read(&p, Some(&suite_dir())).unwrap_err();
        assert_eq!(codes(&e), want, "{name}: {e}");
        seen += 1;
    }
    assert_eq!(seen, bad.len());
}

#[test]
fn absolute_paths_into_the_suite_folder_are_refused() {
    let inside = suite_dir().join("tasks").join("mini-inspect-01").join("start.tar");
    let inside = std::path::absolute(inside).unwrap();
    let inside = inside.to_str().unwrap();
    let flag = format!("--start={inside}");
    for arg in [inside, flag.as_str()] {
        let json = adapter_json(&["agent", "--mcp-config", "{mcp_config}", arg], "stdin");
        let e = Adapter::parse(&json, Some(&suite_dir())).unwrap_err();
        assert_eq!(e.code(), ErrorCode::InvalidAdapter, "{arg}");
        assert!(e.to_string().contains("suite folder"), "{e}");
    }
    // The canonical (link-resolved) spelling is caught too.
    let canon = std::fs::canonicalize(suite_dir()).unwrap().join("tasks");
    let json = adapter_json(&["agent", "--mcp-config", "{mcp_config}", canon.to_str().unwrap()], "stdin");
    assert!(Adapter::parse(&json, Some(&suite_dir())).is_err());
    // A path outside the suite folder is fine.
    let outside = std::path::absolute(root("fixtures/agent/harness/sandbox")).unwrap();
    let json = adapter_json(&["agent", "--mcp-config", "{mcp_config}", outside.to_str().unwrap()], "stdin");
    Adapter::parse(&json, Some(&suite_dir())).unwrap();
}

#[test]
fn resolved_paths_never_point_at_oracles_or_into_the_suite() {
    let a = Adapter::read(&fixture("adapters/exec-example.json"), None).unwrap();
    let mut p = paths();
    p.mcp_config = "/x/oracles/t1/oracle.json".to_string();
    assert_eq!(a.resolve_argv(&p, "go", None).unwrap_err().code(), ErrorCode::InvalidAdapter);
    let mut p = paths();
    p.workdir = std::path::absolute(task_dir()).unwrap().to_str().unwrap().to_string();
    assert_eq!(a.resolve_argv(&p, "go", Some(&suite_dir())).unwrap_err().code(), ErrorCode::InvalidAdapter);
    let mut p = paths();
    p.prompt_file = "/run/../suite/x".to_string();
    assert!(a.resolve_argv(&p, "go", None).is_err());
}

#[test]
fn placeholders_are_substituted_without_a_shell() {
    let json =
        adapter_json(&["agent", "--mcp-config={mcp_config}", "$(rm -rf ~); echo {workdir}", "'{prompt_file}'"], "arg");
    let a = Adapter::parse(&json, None).unwrap();
    let argv = a.resolve_argv(&paths(), "Evaluate PL1; rm -rf /", None).unwrap();
    assert_eq!(
        argv,
        [
            "agent",
            "--mcp-config=/run/agent/mcp-config.json",
            "$(rm -rf ~); echo /run/project",
            "'/run/agent/prompt.txt'",
            "Evaluate PL1; rm -rf /",
        ]
    );
    let env = vec![("PATH".to_string(), "/bin".to_string())];
    let cwd = tmp("noshell");
    let input = LaunchInput {
        paths: paths(),
        prompt: "Evaluate PL1; rm -rf /",
        cwd: &cwd,
        env: &env,
        suite_dir: None,
        limits: Limits { wall_time: Duration::from_secs(1), output_cap: 16 },
    };
    let plan = a.plan(&input).unwrap();
    let cmd = plan.command();
    assert_eq!(cmd.get_program(), "agent");
    let args: Vec<_> = cmd.get_args().map(|s| s.to_str().unwrap().to_string()).collect();
    assert_eq!(args, argv[1..]);
    assert_eq!(plan.stdin, None);
    // An `arg` prompt that would be read as an option is refused.
    assert!(a.resolve_argv(&paths(), "--yolo", None).is_err());
    assert!(a.resolve_argv(&paths(), "", None).is_err());
}

#[test]
fn shell_programs_need_declared_shell_access() {
    let mut v = parse(&adapter_json(&["/bin/bash", "-c", "agent {mcp_config}"], "stdin")).unwrap();
    assert!(Adapter::parse(&to_canonical_bytes(&v).unwrap(), None).is_err());
    if let CValue::Obj(m) = &mut v {
        m.insert("shell".to_string(), CValue::Bool(true));
    }
    let a = Adapter::parse(&to_canonical_bytes(&v).unwrap(), None).unwrap();
    assert!(a.shell);
    for program in ["C:\\Windows\\System32\\cmd.exe", "pwsh", "env", "run.BAT"] {
        assert!(Adapter::parse(&adapter_json(&[program, "{mcp_config}"], "stdin"), None).is_err(), "{program}");
    }
}

#[test]
fn environment_is_scrubbed_to_the_allowlist() {
    let env: Vec<(String, String)> = [
        ("PATH", "/bin"),
        ("HOME", "/run/home"),
        ("ANTHROPIC_API_KEY", "not-a-real-key"),
        ("OPENAI_API_KEY", "not-a-real-key"),
        ("AWS_SECRET_ACCESS_KEY", "not-a-real-key"),
        ("GITHUB_TOKEN", "not-a-real-key"),
        ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let kept = scrub_env(&env);
    let names: Vec<&str> = kept.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, ["HOME", "PATH"]);

    // An allowlisted variable that points into the suite folder fails the plan.
    let a = Adapter::read(&fixture("adapters/exec-example.json"), None).unwrap();
    let suite_abs = std::path::absolute(suite_dir()).unwrap();
    let env = vec![("HOME".to_string(), suite_abs.to_str().unwrap().to_string())];
    let cwd = tmp("env");
    let suite = suite_dir();
    let input = LaunchInput {
        paths: paths(),
        prompt: "go",
        cwd: &cwd,
        env: &env,
        suite_dir: Some(&suite),
        limits: Limits { wall_time: Duration::from_secs(1), output_cap: 16 },
    };
    assert_eq!(a.plan(&input).unwrap_err().code(), ErrorCode::InvalidAdapter);
}

#[test]
fn ac4_token_reports() {
    let exp = load(&fixture("tokens/expected.json"));
    let mut n = 0;
    for (name, want) in obj(&exp) {
        if name.starts_with('_') {
            continue;
        }
        let got = parse_token_report(&std::fs::read(fixture("tokens").join(name)).unwrap());
        match want {
            CValue::Str(s) => {
                assert_eq!(s, "not_reported");
                assert_eq!(got, Tokens::NotReported, "{name}");
                let v = got.to_value();
                let m = obj(&v);
                assert_eq!(m.get("tokens_in"), Some(&CValue::Str("not_reported".to_string())));
                assert_eq!(m.get("tokens_out"), Some(&CValue::Str("not_reported".to_string())));
            }
            CValue::Obj(_) => assert_eq!(got.to_value(), *want, "{name}"),
            other => panic!("{other:?}"),
        }
        n += 1;
    }
    assert_eq!(n, std::fs::read_dir(fixture("tokens")).unwrap().count() - 1);
    // Absent report, and a report that is a folder.
    let d = tmp("tokens");
    assert_eq!(read_token_report(&d, "tokens.json"), Tokens::NotReported);
    std::fs::create_dir(d.join("tokens.json")).unwrap();
    assert_eq!(read_token_report(&d, "tokens.json"), Tokens::NotReported);
    let d2 = tmp("tokens-ok");
    std::fs::copy(fixture("tokens/ok.json"), d2.join("tokens.json")).unwrap();
    assert_eq!(read_token_report(&d2, "tokens.json"), Tokens::Reported { tokens_in: 1200, tokens_out: 345 });
    assert_eq!(read_token_report(&d2, "../tokens.json"), Tokens::NotReported);
}

fn host_env() -> Vec<(String, String)> {
    scrub_env(&std::env::vars().collect::<Vec<_>>())
}

#[test]
fn null_adapter_exits_at_once_without_output() {
    let a = Adapter::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/null.json"), None).unwrap();
    let d = tmp("null");
    // The MCP config does not exist: the null agent never opens it, so it makes no tool call.
    let p = AgentPaths::new(&d.join("prompt.txt"), &d.join("missing-mcp-config.json"), &d).unwrap();
    let env = host_env();
    let suite = suite_dir();
    let input = LaunchInput {
        paths: p,
        prompt: "Evaluate PL1.",
        cwd: &d,
        env: &env,
        suite_dir: Some(&suite),
        limits: Limits { wall_time: Duration::from_secs(60), output_cap: DEFAULT_OUTPUT_CAP },
    };
    let mut plan = a.plan(&input).unwrap();
    assert_eq!(plan.program, "fid-eval-harness");
    plan.program = BIN.to_string();
    assert_eq!(plan.limits.wall_time, Duration::from_secs(60));
    assert_eq!(plan.stdin, None, "prompt delivery is `file`");
    // The library never starts a process (AGENT.HARNESS.SANDBOX REQ-006): the test does.
    let out = plan.command().output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0, "the null agent wrote nothing");
}

#[test]
fn stdin_delivery_and_limits_are_carried_in_the_plan() {
    let a = Adapter::parse(&adapter_json(&["agent", "--mcp-config", "{mcp_config}"], "stdin"), None).unwrap();
    let env = host_env();
    let cwd = tmp("stdin");
    let limits = Limits { wall_time: Duration::from_millis(1500), output_cap: 4096 };
    let input = LaunchInput { paths: paths(), prompt: "Evaluate PL1.", cwd: &cwd, env: &env, suite_dir: None, limits };
    let plan = a.plan(&input).unwrap();
    assert_eq!(plan.stdin.as_deref(), Some("Evaluate PL1.".as_bytes()));
    assert_eq!(plan.limits, limits);
    assert_eq!(plan.args, ["--mcp-config", "/run/agent/mcp-config.json"]);
    let cmd = plan.command();
    let envs: Vec<_> = cmd.get_envs().map(|(k, _)| k.to_str().unwrap().to_string()).collect();
    assert!(envs.iter().all(|k| ENV_ALLOWLIST.iter().any(|a| a.eq_ignore_ascii_case(k))), "{envs:?}");
}
