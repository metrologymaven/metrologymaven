//! Acceptance criteria of spec node AUTO.CLI.OUT (AC-1 .. AC-5), driven through a
//! scripted fake bus. Expected exit codes come from `fixtures/auto/cli/out/cases.json`,
//! hand-labelled from REQ-AUTO.CLI.OUT-002 by `make_cases.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_cli::{exit_codes_markdown, invoke, render_human, Bus, ExitCode, Output, EXIT_CODES_DOC};
use fid_surface::{Envelope, ErrorCode, Outcome};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Case {
    name: String,
    emit: Vec<(String, String)>,
    reply: Envelope,
    exit: u8,
    stdout_lines: usize,
    human: Vec<String>,
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}

fn st(v: &CValue) -> String {
    match v {
        CValue::Str(s) => s.clone(),
        other => panic!("expected string, got {other:?}"),
    }
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        other => panic!("expected number, got {other:?}"),
    }
}

fn cases() -> Vec<Case> {
    let bytes = std::fs::read(root().join("fixtures/auto/cli/out/cases.json")).unwrap();
    let doc = parse(&bytes).unwrap();
    arr(&obj(&doc)["cases"])
        .iter()
        .map(|c| {
            let c = obj(c);
            let expect = obj(&c["expect"]);
            let reply = Envelope::decode(&to_canonical_bytes(&c["reply"]).unwrap()).unwrap();
            Case {
                name: st(&c["name"]),
                emit: arr(&c["emit"])
                    .iter()
                    .map(|e| {
                        let e = arr(e);
                        (st(&e[0]), st(&e[1]))
                    })
                    .collect(),
                reply,
                exit: num(&expect["exit"]) as u8,
                stdout_lines: num(&expect["stdout_lines"]) as usize,
                human: arr(&expect["human"]).iter().map(st).collect(),
            }
        })
        .collect()
}

/// The scripted fake bus: knows `demo show-thing` and replays one case.
struct FakeBus<'c> {
    case: &'c Case,
}

impl Bus for FakeBus<'_> {
    fn resolve(&self, domain: &str, verb_object: &str) -> Option<String> {
        (domain == "demo" && verb_object == "show-thing").then(|| "demo.show_thing@1".to_string())
    }

    fn call(&mut self, command_id: &str, out: &mut Output<'_>) -> Envelope {
        assert_eq!(command_id, "demo.show_thing@1");
        for (kind, msg) in &self.case.emit {
            match kind.as_str() {
                "warn" => out.warn(msg),
                "progress" => out.progress(msg),
                "log" => out.log(msg),
                other => panic!("unknown emit kind {other}"),
            }
        }
        self.case.reply.clone()
    }
}

struct Run {
    exit: ExitCode,
    stdout: String,
    stderr: String,
}

fn run(case: &Case, args: &[&str]) -> Run {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut so, mut se) = (Vec::new(), Vec::new());
    let exit = invoke(&args, &mut FakeBus { case }, &mut so, &mut se);
    Run { exit, stdout: String::from_utf8(so).unwrap(), stderr: String::from_utf8(se).unwrap() }
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no case {name}"))
}

/// Stdout under `--json` is exactly one line of RFC 8785 JSON that decodes as a valid envelope.
fn one_envelope_line(stdout: &str) -> Envelope {
    assert!(stdout.ends_with('\n'), "stdout must end with a newline: {stdout:?}");
    let line = &stdout[..stdout.len() - 1];
    assert!(!line.contains('\n'), "stdout must be exactly one line: {stdout:?}");
    let canonical = to_canonical_bytes(&parse(line.as_bytes()).unwrap()).unwrap();
    assert_eq!(canonical, line.as_bytes(), "stdout is not RFC 8785 canonical");
    let env = Envelope::decode(line.as_bytes()).unwrap();
    env.validate().unwrap();
    env
}

/// AC-1: the R0 fixture command prints one computed envelope line and exits 0.
#[test]
fn ac1_json_computed_one_line_exit_0() {
    let r = run(&case("computed"), &["demo", "show-thing", "--json"]);
    let env = one_envelope_line(&r.stdout);
    assert_eq!(env.outcome, Outcome::Computed);
    assert_eq!(env.command_id, "demo.show_thing@1");
    assert_eq!(r.exit, ExitCode::Success);
    assert_eq!(r.exit.code(), 0);
    assert_eq!(r.stderr, "");
}

/// AC-2: a policy refusal is exit 3 with one failed / policy_refused envelope.
#[test]
fn ac2_policy_refused_exit_3() {
    let r = run(&case("policy refused"), &["demo", "show-thing", "--json"]);
    let env = one_envelope_line(&r.stdout);
    assert_eq!(env.outcome, Outcome::Failed);
    assert_eq!(env.error.unwrap().code, ErrorCode::PolicyRefused);
    assert_eq!(r.exit.code(), 3);
}

/// AC-3: every scripted outcome maps to the exit code of REQ-AUTO.CLI.OUT-002, in both modes.
#[test]
fn ac3_exit_code_per_outcome() {
    let all = cases();
    assert!(all.len() >= 9);
    for c in &all {
        let r = run(c, &["demo", "show-thing", "--json"]);
        assert_eq!(r.exit.code(), c.exit, "{}: --json exit code", c.name);
        assert_eq!(r.stdout.lines().count(), c.stdout_lines, "{}: stdout {:?}", c.name, r.stdout);
        if c.stdout_lines == 1 {
            assert_eq!(one_envelope_line(&r.stdout), c.reply, "{}", c.name);
        }
        assert_eq!(ExitCode::from_envelope(&c.reply).code(), c.exit, "{}: from_envelope", c.name);

        let h = run(c, &["demo", "show-thing"]);
        assert_eq!(h.exit.code(), c.exit, "{}: human exit code", c.name);
        for want in &c.human {
            assert!(h.stdout.contains(want.as_str()), "{}: human output lacks {want:?}: {:?}", c.name, h.stdout);
        }
    }
    // Every code in the table is reached by at least one fixture.
    for code in ExitCode::ALL {
        assert!(all.iter().any(|c| c.exit == code.code()), "no fixture reaches exit {}", code.code());
    }
}

/// AC-4: warnings, progress and logs go to stderr; stdout still holds exactly one line.
#[test]
fn ac4_diagnostics_never_reach_stdout() {
    for name in ["computed with warnings and progress", "command failed"] {
        let c = case(name);
        let r = run(&c, &["demo", "show-thing", "--json"]);
        one_envelope_line(&r.stdout);
        for (_, msg) in &c.emit {
            assert!(r.stderr.contains(msg.as_str()), "{name}: stderr lacks {msg:?}");
            assert!(!r.stdout.contains(msg.as_str()), "{name}: stdout holds {msg:?}");
        }
        let h = run(&c, &["demo", "show-thing"]);
        for (_, msg) in &c.emit {
            assert!(!h.stdout.contains(msg.as_str()), "{name}: human stdout holds {msg:?}");
        }
    }
}

/// AC-5: `docs/cli/exit-codes.md` is generated from the enum and equals the table in code.
/// Set `FID_BLESS=1` to rewrite the page.
#[test]
fn ac5_exit_codes_doc_is_generated() {
    let want = exit_codes_markdown();
    let path = root().join(EXIT_CODES_DOC);
    if std::env::var_os("FID_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(have == want, "{EXIT_CODES_DOC} is out of date. Replace its contents with:\n{want}");
    for c in ExitCode::ALL {
        assert!(have.contains(&format!("| {} | `{}` |", c.code(), c.name())), "{} missing", c.name());
    }
}

/// REQ-AUTO.CLI.OUT-002 / -003: the table is 0..=8, once each, in order.
#[test]
fn exit_table_is_dense_and_unique() {
    for (i, c) in ExitCode::ALL.iter().enumerate() {
        assert_eq!(usize::from(c.code()), i);
    }
    let mut names: Vec<&str> = ExitCode::ALL.iter().map(|c| c.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ExitCode::ALL.len());
}

/// Design, Errors: an unencodable envelope is exit 1, nothing on stdout and an
/// `internal_error` envelope on stderr.
#[test]
fn unencodable_envelope_is_internal_error_on_stderr() {
    let r = run(&case("unencodable envelope"), &["demo", "show-thing", "--json"]);
    assert_eq!(r.exit, ExitCode::Internal);
    assert_eq!(r.stdout, "");
    let env = one_envelope_line(&r.stderr);
    assert_eq!(env.error.unwrap().code, ErrorCode::InternalError);

    let h = run(&case("unencodable envelope"), &["demo", "show-thing"]);
    assert_eq!(h.exit, ExitCode::Internal);
    assert_eq!(h.stdout, "");
    assert!(h.stderr.starts_with("error[internal_error]: "), "{:?}", h.stderr);
}

/// Usage errors are exit 2 and still one envelope under `--json`.
#[test]
fn usage_errors_exit_2() {
    let c = case("computed");
    for args in [
        &["demo", "--json"][..],
        &["demo", "nope", "--json"][..],
        &["demo", "show-thing", "--frobnicate", "--json"][..],
        &["demo", "show-thing", "extra", "--json"][..],
    ] {
        let r = run(&c, args);
        assert_eq!(r.exit, ExitCode::Usage, "{args:?}");
        let env = one_envelope_line(&r.stdout);
        assert_eq!(env.error.unwrap().code, ErrorCode::UsageError, "{args:?}");
    }
}

/// A failing stdout is exit 1 with the reason on stderr.
#[test]
fn stdout_write_failure_is_internal() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut se = Vec::new();
    let env = Envelope::computed("demo.show_thing@1", CValue::Null);
    let code = Output::new(true, &mut Broken, &mut se).finish(&env);
    assert_eq!(code, ExitCode::Internal);
    assert!(String::from_utf8(se).unwrap().contains("internal_error"));
}

/// REQ-AUTO.CLI.OUT-004: human output names the outcome, the journal hash and the error,
/// and text from the bus cannot forge extra lines.
#[test]
fn human_rendering() {
    let hash = format!("sha256:{}", "cd".repeat(32));
    let mut env = Envelope::bare("demo.show_thing@1", Outcome::Committed);
    env.journal_entry_hash = Some(hash.clone());
    assert_eq!(render_human(&env), format!("demo.show_thing@1: committed\njournal entry: {hash}\n"));

    let env = Envelope::failed("demo.show_thing@1", ErrorCode::NotFound, "no such\nfeature\u{1b}[31m", None);
    let text = render_human(&env);
    assert_eq!(text.lines().count(), 2, "{text:?}");
    assert!(text.contains("error[not_found]: no such"));
    assert!(!text.contains('\u{1b}'));
}
