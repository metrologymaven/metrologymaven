//! Acceptance criteria AC-1 and AC-2 of spec node AUTO.CTRL.CLIENT: the `fid ctrl status` and
//! `fid ctrl ping` binaries against the in-process test app (a `fid_ctrl::Server`), and with no
//! app. The library criteria (AC-3 to AC-5) are in `crates/fid-ctrl/tests/client.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_ctrl::{Config, Handler, Log, MemoryLog, ReplyBody, Request, Server};
use fid_surface::{Envelope, ErrorCode, Outcome};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-cli-ctrl-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[derive(Default)]
struct Ops {
    seen: AtomicUsize,
}

impl Handler for Ops {
    fn handle(&self, _req: &Request) -> ReplyBody {
        self.seen.fetch_add(1, Ordering::SeqCst);
        ReplyBody::Event(fid_ctrl::msg::coded_error_event("not_found", "no operations"))
    }
}

fn token(root: &Path) -> PathBuf {
    root.join("profile").join("ctrl.token")
}

fn start(root: &Path, ops: &Arc<Ops>) -> Server {
    let cfg = Config::new("127.0.0.1", &root.join("run"), &token(root));
    Server::start(cfg, ops.clone() as Arc<dyn Handler>, Arc::new(MemoryLog::default()) as Arc<dyn Log>).unwrap()
}

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn fid(args: &[&str], env: &[(&str, &Path)]) -> Ran {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid"));
    cmd.args(args).env_remove("FID_CTRL_RUNTIME_DIR").env_remove("FID_CTRL_TOKEN_FILE");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    Ran {
        code: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

fn one_envelope(r: &Ran) -> Envelope {
    assert!(r.stdout.ends_with('\n') && r.stdout.matches('\n').count() == 1, "not one line: {:?}", r.stdout);
    let env = Envelope::decode(r.stdout.trim_end_matches('\n').as_bytes()).unwrap();
    env.validate().unwrap();
    env
}

fn fields(v: &Option<CValue>) -> BTreeMap<String, CValue> {
    match v {
        Some(CValue::Obj(o)) => o.clone(),
        other => panic!("expected an object, got {other:?}"),
    }
}

fn num(o: &BTreeMap<String, CValue>, k: &str) -> f64 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n,
        other => panic!("{k}: {other:?}"),
    }
}

#[test]
fn ac1_status_json_reports_found_authenticated_version_1() {
    let root = temp_dir("ac1");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    let run = root.join("run");
    let tok = token(&root);
    let r = fid(
        &["ctrl", "status", "--json", "--runtime-dir", run.to_str().unwrap(), "--token-file", tok.to_str().unwrap()],
        &[],
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    let env = one_envelope(&r);
    assert_eq!(env.command_id, "ctrl.status@1");
    assert_eq!(env.outcome, Outcome::Computed);
    let o = fields(&env.result);
    assert_eq!(o.get("found"), Some(&CValue::Bool(true)));
    assert_eq!(o.get("authenticated"), Some(&CValue::Bool(true)));
    assert_eq!(num(&o, "version"), 1.0);
    assert_eq!(o.get("instance_id"), Some(&CValue::Str(server.instance_id().to_string())));
    assert_eq!(num(&o, "port"), f64::from(server.local_addr().port()));

    // The same through the environment variables, in human form.
    let r = fid(&["ctrl", "status"], &[("FID_CTRL_RUNTIME_DIR", &run), ("FID_CTRL_TOKEN_FILE", &tok)]);
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    assert!(r.stdout.starts_with("ctrl.status@1: computed\n"), "{}", r.stdout);
    assert_eq!(ops.seen.load(Ordering::SeqCst), 0, "status ran an operation");
    server.stop().unwrap();
}

#[test]
fn ac2_no_app_exits_6_and_reports_not_found() {
    let root = temp_dir("ac2");
    let run = root.join("run");
    let tok = token(&root);
    let r = fid(
        &["ctrl", "status", "--json", "--runtime-dir", run.to_str().unwrap(), "--token-file", tok.to_str().unwrap()],
        &[],
    );
    assert_eq!(r.code, 6, "stderr: {}", r.stderr);
    let env = one_envelope(&r);
    assert_eq!(env.outcome, Outcome::NotRun);
    let o = fields(&env.result);
    assert_eq!(o.get("found"), Some(&CValue::Bool(false)));
    assert_eq!(o.get("authenticated"), Some(&CValue::Bool(false)));
    assert_eq!(o.get("version"), Some(&CValue::Null));
    assert_eq!(o.get("ctrl_error"), Some(&CValue::Str("app_not_found".into())));

    // Human form and ping: also exit 6.
    let r =
        fid(&["ctrl", "status", "--runtime-dir", run.to_str().unwrap(), "--token-file", tok.to_str().unwrap()], &[]);
    assert_eq!(r.code, 6);
    assert!(r.stdout.contains("not_run"), "{}", r.stdout);
    let r = fid(&["ctrl", "ping", "--runtime-dir", run.to_str().unwrap(), "--token-file", tok.to_str().unwrap()], &[]);
    assert_eq!(r.code, 6);
}

#[test]
fn ping_reports_round_trip_and_runs_no_operation() {
    let root = temp_dir("ping");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    let run = root.join("run");
    let tok = token(&root);
    let r = fid(
        &[
            "ctrl",
            "ping",
            "--json",
            "--count",
            "3",
            "--runtime-dir",
            run.to_str().unwrap(),
            "--token-file",
            tok.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    let env = one_envelope(&r);
    assert_eq!(env.command_id, "ctrl.ping@1");
    assert_eq!(env.outcome, Outcome::Computed);
    let o = fields(&env.result);
    assert!(num(&o, "rtt_ms") >= 0.0);
    assert!(num(&o, "min_ms") <= num(&o, "max_ms"));
    assert_eq!(num(&o, "count"), 3.0);
    assert_eq!(num(&o, "version"), 1.0);
    // REQ-AUTO.CTRL.CLIENT-005: no operation (so no registry command) ran.
    assert_eq!(ops.seen.load(Ordering::SeqCst), 0);
    server.stop().unwrap();
}

#[test]
fn usage_errors_are_exit_2_envelopes() {
    for args in [
        &["ctrl", "status", "--json"][..],
        &["ctrl", "status", "--json", "--runtime-dir"][..],
        &["ctrl", "bogus", "--json"][..],
        &["ctrl", "--json"][..],
        &["ctrl", "status", "--json", "--count", "2", "--runtime-dir", "x", "--token-file", "y"][..],
        &["ctrl", "ping", "--json", "--count", "0", "--runtime-dir", "x", "--token-file", "y"][..],
    ] {
        let r = fid(args, &[]);
        assert_eq!(r.code, 2, "{args:?}: {}", r.stderr);
        let env = one_envelope(&r);
        assert_eq!(env.error.unwrap().code, ErrorCode::UsageError, "{args:?}");
    }
}
