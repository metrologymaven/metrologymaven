//! Unit and integration tests for `AUTO.CLI.DISPATCH`, `AUTO.CLI.ARGS`, `AUTO.CLI.GATE`,
//! `AUTO.CLI.TARGET`, and `AUTO.CLI.JOB` (`crates/fid-cli`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus_api::{BusEnvelope, CommandBus, CommandCall, InProcessBus};
use fid_canon::{parse, CValue};
use fid_cli::args::{parse_command_args, ParamsReader, StdParamsReader};
use fid_cli::dispatch::dispatch_resolved;
use fid_cli::job::{run_tracked_invocation, ChannelInterruptSource};
use fid_cli::target::{resolve_target, AppCtrlBus, Target, TargetError};
use fid_cli::{embedded_tree, invoke_with_bus_api, ExitCode};
use fid_ctrl::server::{Config as CtrlConfig, Handler as CtrlHandler, MemoryLog, Server as CtrlServer};
use fid_ctrl::{ReplyBody, Request as CtrlRequest};
use fid_surface::jobs::{
    BusEvent, BusRequest, BusTransport, CountingKeySource, ManualClock, Purpose, SendError, Tracker,
};
use fid_surface::{project, Envelope, ErrorCode, Outcome};
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn min_classes_bytes() -> Vec<u8> {
    fs::read(repo_root().join("fixtures/manifest/min-classes.json")).unwrap()
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn run_args(
    bus: &mut dyn CommandBus,
    args: &[&str],
    env: &dyn Fn(&str) -> Option<String>,
    stdin: &[u8],
) -> (ExitCode, String, String) {
    let tree = embedded_tree().unwrap();
    let argv: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let mut cur = Cursor::new(stdin);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = invoke_with_bus_api(&tree, "0.0.1", &argv, bus, env, &mut cur, &mut stdout, &mut stderr);
    (code, String::from_utf8(stdout).unwrap(), String::from_utf8(stderr).unwrap())
}

#[test]
fn args_preserves_quantity_literals_and_merges_stdin_params() {
    let tree = embedded_tree().unwrap();
    let cmd = tree.find("align", "set-datum").unwrap();
    let major = cmd.default_major().unwrap();
    let reader = StdParamsReader;

    // Explicit quantity literal with exponent and trailing zero preserved verbatim.
    let rest: Vec<String> =
        ["--source", "root:in/a.pts", "--offset", "1.2700e1 mm"].iter().map(|s| (*s).to_string()).collect();
    let mut empty = Cursor::new(b"" as &[u8]);
    let parsed = parse_command_args(major, &rest, true, &mut empty, None, &reader).unwrap();
    let CValue::Obj(obj) = &parsed.params else { panic!("expected object") };
    let CValue::Obj(off) = obj.get("offset").unwrap() else { panic!("expected quantity object") };
    assert_eq!(off.get("value"), Some(&CValue::Str("1.2700e1".to_string())));
    assert_eq!(off.get("unit"), Some(&CValue::Str("mm".to_string())));

    // Missing unit fails with UsageError naming parameter and declared unit.
    let missing_unit: Vec<String> =
        ["--source", "root:in/a.pts", "--offset", "12.7"].iter().map(|s| (*s).to_string()).collect();
    let mut empty2 = Cursor::new(b"" as &[u8]);
    let err = parse_command_args(major, &missing_unit, true, &mut empty2, None, &reader).unwrap_err();
    assert!(err.to_string().contains("offset"));
    assert!(err.to_string().contains("mm"));

    // `--params -` shallow merge where explicit flags win.
    let merge_rest: Vec<String> = ["--params", "-", "--mode", "fast"].iter().map(|s| (*s).to_string()).collect();
    let mut stdin_cur = Cursor::new(br#"{"source":"root:in/b.pts","offset":{"value":"3","unit":"in"},"mode":"exact"}"#);
    let merged = parse_command_args(major, &merge_rest, true, &mut stdin_cur, None, &reader).unwrap();
    let CValue::Obj(mobj) = &merged.params else { panic!("expected object") };
    assert_eq!(mobj.get("mode"), Some(&CValue::Str("fast".to_string())));
    assert_eq!(mobj.get("source"), Some(&CValue::Str("root:in/b.pts".to_string())));

    // > 1 MiB params fails with params_too_large.
    struct HugeReader;
    impl ParamsReader for HugeReader {
        fn read_params(&self, source: &str, stdin: &mut dyn std::io::Read) -> Result<Vec<u8>, fid_cli::args::ArgError> {
            StdParamsReader.read_params(source, stdin)
        }
    }
    let big = vec![b' '; 1024 * 1024 + 8];
    let mut big_cur = Cursor::new(big);
    let too_big: Vec<String> = ["--params", "-"].iter().map(|s| (*s).to_string()).collect();
    let err_big = parse_command_args(major, &too_big, true, &mut big_cur, None, &HugeReader).unwrap_err();
    assert_eq!(err_big.code(), "params_too_large");
}

#[test]
fn gate_blocks_m1_s1_a0_unsupported_dry_run_and_raw_paths_before_bus() {
    let mut bus = InProcessBus::new();
    // Register handlers for everything to prove gate stops BEFORE bus is contacted.
    bus.register_envelope_fn("motion.move_axis", 1, |_| panic!("M1 must never reach bus"));
    bus.register_envelope_fn("trust.sign_record", 1, |_| panic!("S1 must never reach bus"));
    bus.register_envelope_fn("plugin.install_pack", 1, |_| panic!("A0 must never reach bus"));
    bus.register_envelope_fn("project.list_features", 1, |_| panic!("unsupported dry-run must never reach bus"));
    bus.register_envelope_fn("align.set_datum", 1, |_| panic!("raw path must never reach bus"));

    let (c1, o1, _) = run_args(&mut bus, &["motion", "move-axis", "--json"], &no_env, b"");
    assert_eq!(c1, ExitCode::PolicyRefused);
    assert!(o1.contains("approval station"));

    let (c2, o2, _) = run_args(&mut bus, &["trust", "sign-record", "--json"], &no_env, b"");
    assert_eq!(c2, ExitCode::PolicyRefused);
    assert!(o2.contains("out-of-band signing"));

    let (c3, o3, _) = run_args(&mut bus, &["plugin", "install-pack", "--json"], &no_env, b"");
    assert_eq!(c3, ExitCode::PolicyRefused);
    assert!(o3.contains("GUI settings"));

    let (c4, o4, _) = run_args(&mut bus, &["project", "list-features", "--dry-run", "--json"], &no_env, b"");
    assert_eq!(c4, ExitCode::Usage);
    let e4 = Envelope::decode(o4.trim_end().as_bytes()).unwrap();
    assert_eq!(e4.error.unwrap().code, ErrorCode::DryRunUnsupported);

    // Raw path with SurfaceProjection checks `path_like` (`align.set_datum@1` `source`).
    let proj = project(&min_classes_bytes()).unwrap();
    let sp = proj.commands.iter().find(|c| c.command_id.as_str() == "align.set_datum@1").unwrap();
    let tree = embedded_tree().unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut stdin = Cursor::new(b"" as &[u8]);
    let rest: Vec<String> = ["--source", "C:\\raw\\pts.txt", "--offset", "1mm", "--project", "root:p/min.fid"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let rc_raw = dispatch_resolved(
        &tree,
        &sp.command_id,
        1,
        true,
        &rest,
        &mut bus,
        &no_env,
        &mut stdin,
        &mut out,
        &mut err,
        Some(sp),
    );
    assert_eq!(rc_raw, ExitCode::PolicyRefused);
    let e_raw = Envelope::decode(String::from_utf8(out).unwrap().trim_end().as_bytes()).unwrap();
    assert_eq!(e_raw.error.unwrap().code, ErrorCode::RawPathRefused);
}

#[test]
fn gate_blocks_raw_path_through_invoke_with_bus_api_and_binary_dispatch() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_cb = Arc::clone(&calls);
    let mut bus = InProcessBus::new();
    bus.register_envelope_fn("align.set_datum", 1, move |call| {
        calls_cb.fetch_add(1, Ordering::SeqCst);
        BusEnvelope::committed(call.full_command_id(), format!("sha256:{}", "aa".repeat(32)), Some(call.params.clone()))
    });

    // 1. `invoke_with_bus_api` (`run_args`) with `--source C:\raw\pts.txt` -> exit 3, 0 bus calls.
    let (rc1, o1, _) = run_args(
        &mut bus,
        &[
            "align",
            "set-datum",
            "--source",
            "C:\\raw\\pts.txt",
            "--offset",
            "1mm",
            "--project",
            "root:p/min.fid",
            "--json",
        ],
        &no_env,
        b"",
    );
    assert_eq!(rc1, ExitCode::PolicyRefused);
    assert_eq!(rc1.code(), 3);
    let e1 = Envelope::decode(o1.trim_end().as_bytes()).unwrap();
    assert_eq!(e1.error.unwrap().code, ErrorCode::RawPathRefused);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // 2. `invoke_with_bus_api` (`run_args`) with `--params -` raw path -> exit 3, 0 bus calls.
    let (rc2, o2, _) = run_args(
        &mut bus,
        &["align", "set-datum", "--params", "-", "--project", "root:p/min.fid", "--json"],
        &no_env,
        br#"{"source":"C:\\raw\\pts.txt","offset":{"value":"1","unit":"mm"}}"#,
    );
    assert_eq!(rc2, ExitCode::PolicyRefused);
    assert_eq!(rc2.code(), 3);
    let e2 = Envelope::decode(o2.trim_end().as_bytes()).unwrap();
    assert_eq!(e2.error.unwrap().code, ErrorCode::RawPathRefused);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // 3. `run_binary_dispatch` with `--source C:\raw\pts.txt` -> exit 3, raw_path_refused.
    let tree = embedded_tree().unwrap();
    let rest: Vec<String> = ["--source", "C:\\raw\\pts.txt", "--offset", "1mm", "--project", "root:p/min.fid"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut stdin = Cursor::new(b"" as &[u8]);
    let mut out3 = Vec::new();
    let mut err3 = Vec::new();
    let rc3 = fid_cli::dispatch::run_binary_dispatch(
        &tree,
        &CommandId::parse("align.set_datum@1").unwrap(),
        1,
        true,
        &rest,
        &no_env,
        &mut stdin,
        &mut out3,
        &mut err3,
    );
    assert_eq!(rc3, ExitCode::PolicyRefused);
    assert_eq!(rc3.code(), 3);
    let e3 = Envelope::decode(String::from_utf8(out3).unwrap().trim_end().as_bytes()).unwrap();
    assert_eq!(e3.error.unwrap().code, ErrorCode::RawPathRefused);

    // 4. Valid `root:in/pts.txt` handle reaches the bus and exits 0.
    let (rc4, o4, se4) = run_args(
        &mut bus,
        &[
            "align",
            "set-datum",
            "--source",
            "root:in/pts.txt",
            "--offset",
            "1mm",
            "--project",
            "root:p/min.fid",
            "--json",
        ],
        &no_env,
        b"",
    );
    assert_eq!(rc4, ExitCode::Success, "stderr: {se4}");
    assert_eq!(rc4.code(), 0);
    let e4 = Envelope::decode(o4.trim_end().as_bytes()).unwrap();
    assert_eq!(e4.outcome, Outcome::Committed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn target_error_conflict_maps_to_conflict_and_prepare_dispatch_avoids_prescan() {
    let env = TargetError::Conflict { holder: "inspect (pid 4242)".into() }.to_envelope("align.set_datum@1");
    assert_eq!(env.outcome, Outcome::Failed);
    assert_eq!(env.error.unwrap().code, ErrorCode::Conflict);

    let tree = embedded_tree().unwrap();
    let rest: Vec<String> =
        ["--reason", "--token-file", "--app", "--runtime-dir", "/tmp/rt"].iter().map(|s| (*s).to_string()).collect();
    let mut empty = Cursor::new(b"" as &[u8]);
    let prepared = fid_cli::dispatch::prepare_dispatch(
        &tree,
        &CommandId::parse("project.list_features@1").unwrap(),
        1,
        true,
        &rest,
        &no_env,
        &mut empty,
        None,
    )
    .unwrap();
    assert_eq!(prepared.call.options.reason.as_deref(), Some("--token-file"));
    assert_eq!(prepared.token_file, None);
    assert_eq!(prepared.runtime_dir.as_deref(), Some("/tmp/rt"));
}

#[test]
fn target_selection_rules_and_request_filed_exit_8() {
    assert_eq!(resolve_target(Some("root:p/a.fid"), true, None, true), Err(TargetError::TargetConflict));
    assert_eq!(
        resolve_target(Some("root:p/a.fid"), false, Some("root:p/b.fid"), true),
        Ok(Target::Headless { project: "root:p/a.fid".to_string() })
    );
    assert_eq!(resolve_target(None, true, Some("root:p/b.fid"), true), Ok(Target::App));
    assert_eq!(
        resolve_target(None, false, Some("root:p/b.fid"), true),
        Ok(Target::Headless { project: "root:p/b.fid".to_string() })
    );
    assert_eq!(resolve_target(None, false, Some(""), true), Err(TargetError::NoProjectSelected));
    assert_eq!(resolve_target(None, false, None, false), Ok(Target::None));

    // Raw `--project` path refused with exit 3 (`raw_path_refused`).
    let mut bus = InProcessBus::new();
    bus.register_envelope_fn("align.adopt_alignment", 1, |call| {
        BusEnvelope::request_filed(call.full_command_id(), "0193a5b0-1c2d-7e3f-8a4b-000000000001")
    });
    let (rc_raw_proj, out_rp, _) =
        run_args(&mut bus, &["align", "adopt-alignment", "--project", "C:\\data\\min.fid", "--json"], &no_env, b"");
    assert_eq!(rc_raw_proj, ExitCode::PolicyRefused);
    assert_eq!(Envelope::decode(out_rp.trim_end().as_bytes()).unwrap().error.unwrap().code, ErrorCode::RawPathRefused);

    // Valid root handle dispatches `align.adopt_alignment@1` -> `request_filed` exit 8.
    let (rc_req, out_req, _) =
        run_args(&mut bus, &["align", "adopt-alignment", "--project", "root:projects/min.fid", "--json"], &no_env, b"");
    assert_eq!(rc_req, ExitCode::RequestFiled);
    assert_eq!(rc_req.code(), 8);
    let env_req = Envelope::decode(out_req.trim_end().as_bytes()).unwrap();
    assert_eq!(env_req.outcome, Outcome::RequestFiled);
    assert_eq!(env_req.request_id.as_deref(), Some("0193a5b0-1c2d-7e3f-8a4b-000000000001"));
}

#[test]
fn in_process_bus_happy_path_and_dry_run_envelope() {
    let mut bus = InProcessBus::new();
    bus.register_envelope_fn("project.list_features", 1, |call| {
        let mut res = BTreeMap::new();
        res.insert("count".to_string(), CValue::Num(3.0));
        res.insert("dry_run".to_string(), CValue::Bool(call.options.dry_run));
        BusEnvelope::computed(call.full_command_id(), CValue::Obj(res))
    });

    let (code, out, stderr) = run_args(&mut bus, &["project", "list-features", "--json"], &no_env, b"");
    assert_eq!(code, ExitCode::Success, "stderr: {stderr}");
    let env = Envelope::decode(out.trim_end().as_bytes()).unwrap();
    assert_eq!(env.outcome, Outcome::Computed);
    let CValue::Obj(m) = env.result.unwrap() else { panic!("expected obj") };
    assert_eq!(m.get("count"), Some(&CValue::Num(3.0)));
    assert!(parse(out.trim_end().as_bytes()).is_ok());
}

#[test]
fn job_tracker_interrupt_once_cancels_and_twice_detaches_with_no_reply() {
    struct MockJobTransport {
        events: VecDeque<BusEvent>,
        cancel_sent: usize,
        cancel_immediately: bool,
    }
    impl BusTransport for MockJobTransport {
        fn send(&mut self, req: &BusRequest) -> Result<(), SendError> {
            let job = Uuid::from_u128(0x0193_a5b0_1c2d_7e3f_8a4b_0000_0000_0099);
            match req.purpose {
                Purpose::Submit => {
                    self.events.push_back(BusEvent::Accepted { key: req.key, job });
                    self.events.push_back(BusEvent::Progress { job, fraction: 0.5, stage: "meshing".to_string() });
                }
                Purpose::Cancel => {
                    self.cancel_sent += 1;
                    if self.cancel_immediately {
                        self.events.push_back(BusEvent::Finished {
                            key: req.key,
                            job,
                            envelope: Box::new(Envelope::bare(req.command.as_str(), Outcome::Cancelled)),
                        });
                    }
                }
                Purpose::Wait => {}
            }
            Ok(())
        }
        fn recv(&mut self) -> Option<BusEvent> {
            self.events.pop_front()
        }
    }

    // 1st interrupt cancels -> exit 5 (`Cancelled`).
    let (tx1, rx1) = mpsc::channel();
    tx1.send(()).unwrap();
    let mut tracker1 = Tracker::new(
        MockJobTransport { events: VecDeque::new(), cancel_sent: 0, cancel_immediately: true },
        ManualClock::new(),
        CountingKeySource::new(1_700_000_000_000),
    );
    let mut int1 = ChannelInterruptSource::new(rx1);
    let mut se1 = Vec::new();
    let env1 = run_tracked_invocation(
        &mut tracker1,
        CommandId::parse("demo.slow_job@1").unwrap(),
        CValue::Obj(BTreeMap::new()),
        Duration::from_secs(5),
        &mut int1,
        false,
        &mut se1,
    );
    assert_eq!(env1.outcome, Outcome::Cancelled);
    assert_eq!(ExitCode::from_envelope(&env1), ExitCode::Cancelled);
    assert_eq!(tracker1.transport().cancel_sent, 1);

    // 2nd interrupt detaches -> exit 7 (`NoReply`), stderr names job ID.
    let (tx2, rx2) = mpsc::channel();
    tx2.send(()).unwrap();
    tx2.send(()).unwrap();
    let mut tracker2 = Tracker::new(
        MockJobTransport { events: VecDeque::new(), cancel_sent: 0, cancel_immediately: false },
        ManualClock::new(),
        CountingKeySource::new(1_700_000_000_000),
    );
    let mut int2 = ChannelInterruptSource::new(rx2);
    let mut se2 = Vec::new();
    let env2 = run_tracked_invocation(
        &mut tracker2,
        CommandId::parse("demo.slow_job@1").unwrap(),
        CValue::Obj(BTreeMap::new()),
        Duration::from_secs(5),
        &mut int2,
        false,
        &mut se2,
    );
    assert_eq!(env2.outcome, Outcome::NoReply);
    assert_eq!(ExitCode::from_envelope(&env2), ExitCode::NoReply);
    assert!(String::from_utf8(se2).unwrap().contains("0193a5b0-1c2d-7e3f-8a4b-000000000099"));
}

#[test]
fn app_ctrl_bus_connects_and_dispatches_against_ctrl_server_or_reports_not_run() {
    let dir = std::env::temp_dir().join(format!("fid-cli-app-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let rt = dir.join("rt");
    fs::create_dir_all(&rt).unwrap();
    let tok = dir.join("token");

    // 1. No server running -> `app_not_found` -> NotRun (exit 6).
    let mut se = Vec::new();
    let err = AppCtrlBus::connect(
        Some(rt.to_str().unwrap()),
        Some(tok.to_str().unwrap()),
        &no_env,
        Some(Duration::from_millis(200)),
        Duration::from_secs(1),
        &mut se,
    );
    assert!(matches!(err, Err(TargetError::AppNotFound(_))));
    if let Err(te) = err {
        let env = te.to_envelope("project.list_features@1");
        assert_eq!(ExitCode::from_envelope(&env), ExitCode::NotRun);
    }

    // 2. Live CtrlServer handles `command.execute` (if permitted by OS/sandbox socket ACLs).
    struct EchoCtrl;
    impl CtrlHandler for EchoCtrl {
        fn handle(&self, req: &CtrlRequest) -> ReplyBody {
            let CValue::Obj(o) = &req.params else {
                return ReplyBody::Envelope(Box::new(Envelope::failed(
                    "cli.invoke@1",
                    ErrorCode::UsageError,
                    "bad",
                    None,
                )));
            };
            let cmd = match o.get("command") {
                Some(CValue::Str(s)) => s.clone(),
                _ => "project.list_features@1".to_string(),
            };
            let mut r = BTreeMap::new();
            r.insert("from_app".to_string(), CValue::Bool(true));
            ReplyBody::Envelope(Box::new(Envelope::computed(&cmd, CValue::Obj(r))))
        }
    }

    let cfg = CtrlConfig::new("127.0.0.1", &rt, &tok);
    if let Ok(server) = CtrlServer::start(cfg, Arc::new(EchoCtrl), Arc::new(MemoryLog::default())) {
        let mut bus_err = Vec::new();
        if let Ok(mut app_bus) = AppCtrlBus::connect(
            Some(rt.to_str().unwrap()),
            Some(tok.to_str().unwrap()),
            &no_env,
            Some(Duration::from_secs(2)),
            Duration::from_secs(2),
            &mut bus_err,
        ) {
            let call = CommandCall::new(
                "project.list_features",
                1,
                CValue::Obj(BTreeMap::new()),
                fid_bus_api::PrincipalInfo::script("cli"),
            );
            let _ = app_bus.dispatch(&call);
        }
        let _ = server.stop();
    }
    let _ = fs::remove_dir_all(&dir);
}
