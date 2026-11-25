//! Acceptance criteria of spec node AUTO.CTRL.CLIENT for the client library (AC-3, AC-4, AC-5)
//! and its requirements, against scripted fake servers and the in-process test app. The CLI
//! criteria (AC-1, AC-2) are in `crates/fid-compose/tests/ctrl.rs`. Every listener binds port 0;
//! every token file and runtime directory lives in its own temp directory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_ctrl::auth::{server_proof, verify_client_proof};
use fid_ctrl::client::{self, ClientError, CtrlClient, Target};
use fid_ctrl::frame::{Frame, LineReader};
use fid_ctrl::msg::{auth_event, auth_params, coded_error_event, hello_event, parse_request};
use fid_ctrl::{discovery, Config, Discovery, Handler, Log, MemoryLog, Nonce, Proof, Reply, ReplyBody, Request};
use fid_ctrl::{AuthError, Server, Token, TokenFile};
use fid_surface::jobs::{InstantClock, Reason, SystemKeySource, Tracker};
use fid_surface::{Envelope, Outcome};
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const T: Duration = Duration::from_secs(20);

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-ctrl-client-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

// ------------------------------------------------------------------ scripted fake server

/// A loopback listener with a discovery file and a token file, served by a script.
struct Fake {
    root: PathBuf,
    listener: TcpListener,
    token_path: PathBuf,
    instance: String,
}

impl Fake {
    fn new(name: &str) -> Fake {
        let root = temp_dir(name);
        let token_path = root.join("profile").join("ctrl.token");
        TokenFile::ensure(&token_path).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let instance = "fake0123456789abcdef".to_string();
        let record = Discovery {
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: listener.local_addr().unwrap().port(),
            pid: std::process::id(),
            instance_id: instance.clone(),
            versions: vec![1],
        };
        discovery::write(&root.join("run"), &record).unwrap();
        Fake { root, listener, token_path, instance }
    }

    fn target(&self) -> Target {
        Target {
            runtime_dir: self.root.join("run"),
            token_path: self.token_path.clone(),
            instance_id: None,
            timeout: T,
        }
    }

    fn token(&self) -> Token {
        Token::from_bytes(std::fs::read(&self.token_path).unwrap().try_into().unwrap())
    }

    /// Serves one connection with `script` on a thread.
    fn serve<R: Send + 'static>(&self, script: impl FnOnce(&mut Conn) -> R + Send + 'static) -> JoinHandle<R> {
        let listener = self.listener.try_clone().unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(T)).unwrap();
            let mut conn = Conn { reader: LineReader::new(stream.try_clone().unwrap()), writer: stream };
            script(&mut conn)
        })
    }
}

struct Conn {
    reader: LineReader<TcpStream>,
    writer: TcpStream,
}

impl Conn {
    /// The next request, or `None` at end of stream.
    fn next(&mut self) -> Option<Request> {
        match self.reader.next_line(1024 * 1024).unwrap() {
            Frame::Line(l) => Some(parse_request(&l).unwrap()),
            Frame::Eof => None,
            other => panic!("fake server read {other:?}"),
        }
    }

    fn send(&mut self, id: u64, event: CValue) {
        let r = Reply { v: 1, id: Some(id), body: ReplyBody::Event(event) };
        self.writer.write_all(&r.encode().unwrap()).unwrap();
    }

    /// Answers `hello` with `instance` and a fresh nonce.
    fn hello(&mut self, instance: &str) -> Nonce {
        let req = self.next().unwrap();
        assert_eq!(req.op.as_str(), "hello");
        let nonce = Nonce::random().unwrap();
        self.send(req.id, hello_event(1, instance, Some(&nonce.to_hex())));
        nonce
    }

    /// Reads `auth`, checks the client proof, and answers with `server` (a function of the
    /// client nonce). Returns the client nonce.
    fn auth(&mut self, token: &Token, instance: &str, sn: &Nonce, server: impl FnOnce(&Nonce) -> Proof) -> Nonce {
        let req = self.next().unwrap();
        assert_eq!(req.op.as_str(), "auth");
        let (cn, proof) = auth_params(&req).unwrap();
        assert!(verify_client_proof(token, instance, sn, &cn, &proof), "client proof does not verify");
        self.send(req.id, auth_event(&server(&cn).to_hex()));
        cn
    }

    /// A correct handshake.
    fn login(&mut self, token: &Token, instance: &str) {
        let sn = self.hello(instance);
        self.auth(token, instance, &sn, |cn| server_proof(token, instance, cn, &sn));
    }

    /// Every request until the client closes.
    fn rest(&mut self) -> Vec<String> {
        let mut ops = Vec::new();
        while let Some(r) = self.next() {
            ops.push(r.op.to_string());
        }
        ops
    }
}

fn accepted(job: &Uuid) -> CValue {
    let mut o = BTreeMap::new();
    o.insert("kind".to_string(), CValue::Str("accepted".into()));
    o.insert("job".to_string(), CValue::Str(job.to_string()));
    CValue::Obj(o)
}

fn param_str<'a>(r: &'a Request, k: &str) -> &'a str {
    match &r.params {
        CValue::Obj(o) => match o.get(k) {
            Some(CValue::Str(s)) => s,
            other => panic!("{k}: {other:?}"),
        },
        _ => panic!("params"),
    }
}

// ------------------------------------------------------------------ AC-3: wrong server proof

#[test]
fn ac3_wrong_server_proof_is_server_auth_failed_and_nothing_follows() {
    let fake = Fake::new("ac3");
    let token = fake.token();
    let instance = fake.instance.clone();
    let server = fake.serve(move |c| {
        let sn = c.hello(&instance);
        c.auth(&token, &instance, &sn, |_| Proof([0x5a; 32]));
        c.rest()
    });
    let err = client::connect(&fake.target()).unwrap_err();
    assert_eq!(err, ClientError::ServerAuthFailed);
    assert_eq!(err.code(), "server_auth_failed");
    assert_eq!(server.join().unwrap(), Vec::<String>::new(), "the fake server received an operation after auth");
}

#[test]
fn server_proof_for_another_instance_or_malformed_is_refused() {
    // A proof made with the right token for another instance ID (a relay) is wrong too.
    let fake = Fake::new("relay");
    let token = fake.token();
    let instance = fake.instance.clone();
    let server = fake.serve(move |c| {
        let sn = c.hello(&instance);
        c.auth(&token, &instance, &sn, |cn| server_proof(&token, "another-instance", cn, &sn));
        c.rest()
    });
    assert_eq!(client::connect(&fake.target()).unwrap_err(), ClientError::ServerAuthFailed);
    assert!(server.join().unwrap().is_empty());

    // An `auth` reply without a proof.
    let fake = Fake::new("noproof");
    let instance = fake.instance.clone();
    let server = fake.serve(move |c| {
        c.hello(&instance);
        let req = c.next().unwrap();
        let mut o = BTreeMap::new();
        o.insert("kind".to_string(), CValue::Str("auth".into()));
        o.insert("proof".to_string(), CValue::Str("not hex".into()));
        c.send(req.id, CValue::Obj(o));
        c.rest()
    });
    assert_eq!(client::connect(&fake.target()).unwrap_err(), ClientError::ServerAuthFailed);
    assert!(server.join().unwrap().is_empty());
}

#[test]
fn hello_from_another_instance_closes_before_auth() {
    let fake = Fake::new("mismatch");
    let server = fake.serve(move |c| {
        c.hello("some-other-instance");
        c.rest()
    });
    let err = client::connect(&fake.target()).unwrap_err();
    assert_eq!(err, ClientError::InstanceMismatch);
    assert_eq!(server.join().unwrap(), Vec::<String>::new(), "auth was sent to the wrong instance");
}

#[test]
fn server_error_during_handshake_is_reported() {
    let fake = Fake::new("refused");
    let instance = fake.instance.clone();
    let server = fake.serve(move |c| {
        c.hello(&instance);
        let req = c.next().unwrap();
        c.send(req.id, coded_error_event("auth_failed", "wrong proof"));
        c.rest()
    });
    let err = client::connect(&fake.target()).unwrap_err();
    assert_eq!(err, ClientError::ServerRefused("auth_failed"));
    server.join().unwrap();
}

// ------------------------------------------------------------------ AC-4: disconnect

fn tracker(c: CtrlClient) -> Tracker<CtrlClient, InstantClock, SystemKeySource> {
    Tracker::new(c, InstantClock::new(), SystemKeySource::new())
}

fn cmd() -> CommandId {
    CommandId::parse("test.run_long@1").unwrap()
}

fn empty() -> CValue {
    CValue::Obj(BTreeMap::new())
}

fn pump_until_done(
    t: &mut Tracker<CtrlClient, InstantClock, SystemKeySource>,
    ids: &[fid_surface::jobs::InvocationId],
) {
    let start = Instant::now();
    while ids.iter().any(|id| t.report(*id).is_none()) {
        t.pump(&mut ());
        assert!(start.elapsed() < T, "the tracker did not conclude");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn ac4_drop_reports_no_reply_with_job_for_the_accepted_request() {
    let fake = Fake::new("ac4");
    let token = fake.token();
    let instance = fake.instance.clone();
    let job = Uuid::parse("018f2b6e-0000-7000-8000-000000000001").unwrap();
    let server = fake.serve(move |c| {
        c.login(&token, &instance);
        let reqs: Vec<Request> = (0..3).map(|_| c.next().unwrap()).collect();
        for r in &reqs {
            assert_eq!(r.op.as_str(), "command.execute");
            assert_eq!(param_str(r, "command"), "test.run_long@1");
        }
        c.send(reqs[0].id, accepted(&job));
        // The drop: no more replies, connection closed.
        c.writer.shutdown(Shutdown::Both).unwrap();
        reqs.iter().map(|r| param_str(r, "key").to_string()).collect::<Vec<_>>()
    });
    let mut t = tracker(client::connect(&fake.target()).unwrap());
    let ids: Vec<_> = (0..3).map(|_| t.submit(cmd(), empty(), T).unwrap()).collect();
    let keys = server.join().unwrap();
    // The idempotency key on the wire is the tracker's own.
    for (id, key) in ids.iter().zip(&keys) {
        assert_eq!(t.key(*id).unwrap().to_string(), *key);
    }
    pump_until_done(&mut t, &ids);

    let first = t.report(ids[0]).unwrap();
    assert_eq!(first.envelope.outcome, Outcome::NoReply);
    assert_eq!(first.envelope.job_id.as_deref(), Some(job.to_string().as_str()));
    assert_eq!(first.reason, Reason::TransportClosed);
    // Sent, never accepted, never refused: the server may have run them, so they are
    // `no_reply` without a job ID, never `not_run` (REQ-AUTO.SURF-007; leaf Open questions).
    for id in &ids[1..] {
        let r = t.report(*id).unwrap();
        assert_eq!(r.envelope.outcome, Outcome::NoReply, "{r:?}");
        assert_eq!(r.envelope.job_id, None);
    }
    // After the drop nothing can leave: a new request is `not_run` at once.
    assert!(t.transport().is_closed());
    let late = t.submit(cmd(), empty(), T).unwrap();
    let r = t.report(late).unwrap();
    assert_eq!(r.envelope.outcome, Outcome::NotRun);
    assert_eq!(r.reason, Reason::NotSent);
}

#[test]
fn ac4_refused_before_acceptance_is_not_run_and_final_envelopes_pass_through() {
    let fake = Fake::new("refuse");
    let token = fake.token();
    let instance = fake.instance.clone();
    let job = Uuid::parse("018f2b6e-0000-7000-8000-000000000002").unwrap();
    let server = fake.serve(move |c| {
        c.login(&token, &instance);
        let reqs: Vec<Request> = (0..3).map(|_| c.next().unwrap()).collect();
        // First: accepted, progress, then its final envelope.
        c.send(reqs[0].id, accepted(&job));
        let mut p = BTreeMap::new();
        p.insert("kind".to_string(), CValue::Str("progress".into()));
        p.insert("fraction".to_string(), CValue::Num(0.5));
        p.insert("stage".to_string(), CValue::Str("half".into()));
        c.send(reqs[0].id, CValue::Obj(p));
        let env =
            Envelope { job_id: Some(job.to_string()), ..Envelope::computed("test.run_long@1", CValue::Bool(true)) };
        let r = Reply { v: 1, id: Some(reqs[0].id), body: ReplyBody::Envelope(Box::new(env)) };
        c.writer.write_all(&r.encode().unwrap()).unwrap();
        // Second: explicitly refused before dispatch (an allow-listed code).
        c.send(reqs[1].id, coded_error_event("malformed_message", "bad params"));
        // Third: accepted, then the drop.
        let job3 = Uuid::parse("018f2b6e-0000-7000-8000-000000000003").unwrap();
        c.send(reqs[2].id, accepted(&job3));
        c.writer.shutdown(Shutdown::Both).unwrap();
    });
    let mut t = tracker(client::connect(&fake.target()).unwrap());
    let ids: Vec<_> = (0..3).map(|_| t.submit(cmd(), empty(), T).unwrap()).collect();
    server.join().unwrap();
    let mut progress = Vec::new();
    let start = Instant::now();
    while ids.iter().any(|id| t.report(*id).is_none()) {
        t.pump(&mut progress);
        assert!(start.elapsed() < T);
        thread::sleep(Duration::from_millis(2));
    }
    let r0 = t.report(ids[0]).unwrap();
    assert_eq!((r0.envelope.outcome.clone(), r0.reason.clone()), (Outcome::Computed, Reason::Final));
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].1.stage, "half");
    let r1 = t.report(ids[1]).unwrap();
    assert_eq!(r1.envelope.outcome, Outcome::NotRun);
    assert_eq!(r1.reason, Reason::Refused("malformed_message".to_string()));
    let r2 = t.report(ids[2]).unwrap();
    assert_eq!(r2.envelope.outcome, Outcome::NoReply);
    assert_eq!(r2.envelope.job_id.as_deref(), Some("018f2b6e-0000-7000-8000-000000000003"));
}

// ------------------------------------------------------------------ AC-5 and REQ-001: loopback only

fn write_discovery(dir: &Path, address: IpAddr, port: u16) {
    let record = Discovery { address, port, pid: 1, instance_id: "planted".into(), versions: vec![1] };
    discovery::write(dir, &record).unwrap();
}

#[test]
fn ac5_non_loopback_discovery_is_refused_without_connecting() {
    for addr in ["10.0.0.1", "0.0.0.0", "127.0.0.2", "::", "192.168.1.1"] {
        let root = temp_dir(&format!("ac5-{}", addr.replace([':', '.'], "_")));
        // A listener on 127.0.0.1 at the same port: `0.0.0.0` would reach it if dialled.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        write_discovery(&root.join("run"), addr.parse().unwrap(), port);
        let path = root.join("run").join(Discovery::file_name("planted"));
        let err = CtrlClient::connect(&path, &root.join("none.token"), T).unwrap_err();
        assert_eq!(err.code(), "ctrl_non_loopback", "{addr}");
        let target = Target {
            runtime_dir: root.join("run"),
            token_path: root.join("none.token"),
            instance_id: None,
            timeout: T,
        };
        assert_eq!(client::connect(&target).unwrap_err().code(), "ctrl_non_loopback", "{addr}");
        thread::sleep(Duration::from_millis(50));
        assert!(listener.accept().is_err(), "{addr}: the client connected");
    }
}

#[test]
fn no_discovery_file_or_nothing_listening_is_app_not_found() {
    let root = temp_dir("notfound");
    let target = Target { runtime_dir: root.join("run"), token_path: root.join("t"), instance_id: None, timeout: T };
    assert_eq!(client::connect(&target).unwrap_err().code(), "app_not_found");
    let s = client::status(&target);
    assert!(!s.found && !s.authenticated && s.version.is_none());
    // A stale file: its app is gone and nothing listens at its port.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    write_discovery(&root.join("run"), IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    assert_eq!(client::connect(&target).unwrap_err().code(), "app_not_found");
    // An instance ID that is not a safe file name.
    let bad = Target { instance_id: Some("../x".into()), ..target };
    assert_eq!(client::connect(&bad).unwrap_err().code(), "app_not_found");
}

// ------------------------------------------------------------------ against the test app

/// Counts every request that reaches the operation handler.
#[derive(Default)]
struct Ops {
    seen: AtomicUsize,
}

impl Handler for Ops {
    fn handle(&self, _req: &Request) -> ReplyBody {
        self.seen.fetch_add(1, Ordering::SeqCst);
        ReplyBody::Event(coded_error_event("not_found", "no operations in this test app"))
    }
}

fn start(root: &Path, ops: &Arc<Ops>) -> Server {
    let cfg = Config::new("127.0.0.1", &root.join("run"), &root.join("profile").join("ctrl.token"));
    let log = Arc::new(MemoryLog::default());
    Server::start(cfg, ops.clone() as Arc<dyn Handler>, log as Arc<dyn Log>).unwrap()
}

fn app_target(root: &Path) -> Target {
    Target {
        runtime_dir: root.join("run"),
        token_path: root.join("profile").join("ctrl.token"),
        instance_id: None,
        timeout: T,
    }
}

#[test]
fn status_and_ping_against_the_test_app_run_no_operation() {
    let root = temp_dir("app");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    let s = client::status(&app_target(&root));
    assert_eq!(s.error, None);
    assert!(s.found && s.authenticated);
    assert_eq!(s.version, Some(1));
    assert_eq!(s.instance_id.as_deref(), Some(server.instance_id()));
    assert_eq!(s.address, Some(server.local_addr()));

    let mut c = client::connect(&app_target(&root)).unwrap();
    for _ in 0..3 {
        let rtt = c.ping(T).unwrap();
        assert!(rtt < T);
    }
    c.close();
    // REQ-AUTO.CTRL.CLIENT-005: the ping reaches no operation handler, so no registry command.
    assert_eq!(ops.seen.load(Ordering::SeqCst), 0);
    server.stop().unwrap();
}

#[test]
fn token_is_read_for_every_connection_across_rotations() {
    let root = temp_dir("rotate");
    let ops = Arc::new(Ops::default());
    let first = start(&root, &ops);
    client::connect(&app_target(&root)).unwrap().close();
    first.stop().unwrap();
    // A restart rotates the token; a client that cached it would fail now.
    let second = start(&root, &ops);
    client::connect(&app_target(&root)).unwrap().close();
    // With a stale discovery file of the first instance gone, the second instance is found.
    let s = client::status(&app_target(&root));
    assert_eq!(s.instance_id.as_deref(), Some(second.instance_id()));
    second.stop().unwrap();
}

#[test]
fn instance_selection_and_stale_files_are_skipped() {
    let root = temp_dir("select");
    let ops = Arc::new(Ops::default());
    // A stale file that sorts first.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let stale = Discovery {
        address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
        pid: 1,
        instance_id: "0000stale".into(),
        versions: vec![1],
    };
    discovery::write(&root.join("run"), &stale).unwrap();
    let server = start(&root, &ops);
    let c = client::connect(&app_target(&root)).unwrap();
    assert_eq!(c.discovery().instance_id, server.instance_id());
    c.close();
    let only = Target { instance_id: Some(server.instance_id().to_string()), ..app_target(&root) };
    client::connect(&only).unwrap().close();
    let stale_only = Target { instance_id: Some("0000stale".into()), ..app_target(&root) };
    assert_eq!(client::connect(&stale_only).unwrap_err().code(), "app_not_found");
    server.stop().unwrap();
}

#[test]
fn missing_token_file_is_token_io_and_the_client_creates_nothing() {
    let root = temp_dir("notoken");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    let target = Target { token_path: root.join("profile").join("other.token"), ..app_target(&root) };
    assert_eq!(client::connect(&target).unwrap_err(), ClientError::Token(AuthError::TokenIo));
    assert!(!target.token_path.exists());
    server.stop().unwrap();
}

/// Unix: a group-readable token file or a symlink is refused with `token_permissions`, and the
/// client never replaces the app's file.
#[cfg(unix)]
#[test]
fn token_file_too_open_or_a_link_is_refused_and_left_alone() {
    use std::os::unix::fs::PermissionsExt;
    let root = temp_dir("perm");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    let token = root.join("profile").join("ctrl.token");
    let before = std::fs::read(&token).unwrap();
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert_eq!(client::connect(&app_target(&root)).unwrap_err(), ClientError::Token(AuthError::TokenPermissions));
    assert_eq!(std::fs::read(&token).unwrap(), before, "the client replaced the token");
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();

    let link = root.join("profile").join("link.token");
    std::os::unix::fs::symlink(&token, &link).unwrap();
    let target = Target { token_path: link, ..app_target(&root) };
    assert_eq!(client::connect(&target).unwrap_err(), ClientError::Token(AuthError::TokenPermissions));
    server.stop().unwrap();
}

/// The token never appears in any error text the client produces.
#[test]
fn errors_never_carry_the_token() {
    let fake = Fake::new("leak");
    let token = fake.token();
    let raw = std::fs::read(&fake.token_path).unwrap();
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let instance = fake.instance.clone();
    let server = fake.serve(move |c| {
        let sn = c.hello(&instance);
        c.auth(&token, &instance, &sn, |_| Proof([0; 32]));
        c.rest()
    });
    let err = client::connect(&fake.target()).unwrap_err();
    server.join().unwrap();
    let text = format!("{err} {err:?}");
    assert!(!text.contains(&hex), "{text}");
}

// ------------------------------------------------------------------ REQ-AUTO.SURF-007: taken errors

/// Error codes outside the pre-dispatch allow-list, and a final reply over its bound, leave the
/// request "taken": it may have run, so a drop reports `no_reply`, never `not_run`.
#[test]
fn errors_outside_the_allow_list_and_oversized_finals_are_no_reply() {
    let fake = Fake::new("taken");
    let token = fake.token();
    let instance = fake.instance.clone();
    let server = fake.serve(move |c| {
        c.login(&token, &instance);
        let reqs: Vec<Request> = (0..3).map(|_| c.next().unwrap()).collect();
        c.send(reqs[0].id, coded_error_event("message_too_large", "too large"));
        c.send(reqs[1].id, coded_error_event("raw_path_refused", "raw path"));
        // A final envelope line of 2 MiB: over the 1 MiB bound of an execute reply.
        let big = "x".repeat(2 * 1024 * 1024);
        let env = Envelope::computed("test.run_long@1", CValue::Str(big));
        let r = Reply { v: 1, id: Some(reqs[2].id), body: ReplyBody::Envelope(Box::new(env)) };
        c.writer.write_all(&r.encode().unwrap()).unwrap();
        c.writer.shutdown(Shutdown::Both).unwrap();
    });
    let mut t = tracker(client::connect(&fake.target()).unwrap());
    let ids: Vec<_> = (0..3).map(|_| t.submit(cmd(), empty(), T).unwrap()).collect();
    server.join().unwrap();
    pump_until_done(&mut t, &ids);
    for id in &ids {
        let r = t.report(*id).unwrap();
        assert_eq!(r.envelope.outcome, Outcome::NoReply, "{r:?}");
        assert_eq!(r.envelope.job_id, None);
    }
    assert!(t.transport().counters().malformed >= 1, "the oversized final was not dropped");
}

// ------------------------------------------------------------------ candidates and --instance

/// A candidate that answers with another instance ID or a wrong server proof is skipped, and the
/// next one is used.
#[test]
fn mismatching_and_bad_proof_candidates_are_skipped() {
    let root = temp_dir("skip");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    let mut fakes = Vec::new();
    for (name, wrong_instance) in [("-0mismatch", true), ("-1badproof", false)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let record = Discovery {
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: listener.local_addr().unwrap().port(),
            pid: 1,
            instance_id: name.into(),
            versions: vec![1],
        };
        discovery::write(&root.join("run"), &record).unwrap();
        fakes.push(thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(T)).unwrap();
            let mut c = Conn { reader: LineReader::new(stream.try_clone().unwrap()), writer: stream };
            if wrong_instance {
                c.hello("someone-else");
            } else {
                c.hello(name);
                let req = c.next().unwrap();
                assert_eq!(req.op.as_str(), "auth");
                c.send(req.id, auth_event(&Proof([7; 32]).to_hex()));
            }
            c.rest()
        }));
    }
    let c = client::connect(&app_target(&root)).unwrap();
    assert_eq!(c.discovery().instance_id, server.instance_id());
    c.close();
    for f in fakes {
        assert!(f.join().unwrap().is_empty());
    }
    server.stop().unwrap();
}

#[test]
fn instance_option_refuses_a_record_for_another_instance() {
    let root = temp_dir("inst");
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let record = Discovery {
        address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
        pid: 1,
        instance_id: "bbbb".into(),
        versions: vec![1],
    };
    let written = discovery::write(&root.join("run"), &record).unwrap();
    std::fs::rename(&written, root.join("run").join(Discovery::file_name("aaaa"))).unwrap();
    let target = Target {
        runtime_dir: root.join("run"),
        token_path: root.join("t"),
        instance_id: Some("aaaa".into()),
        timeout: T,
    };
    assert_eq!(client::connect(&target).unwrap_err(), ClientError::InstanceMismatch);
}

// ------------------------------------------------------------------ runtime directory trust (Unix)

#[cfg(unix)]
fn chmod(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
#[test]
fn group_writable_runtime_dir_is_unsafe() {
    let root = temp_dir("gw");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    chmod(&root.join("run"), 0o770);
    let err = client::connect(&app_target(&root)).unwrap_err();
    assert_eq!(err.code(), "ctrl_unsafe_runtime_dir", "{err}");
    let path = server.discovery_path().to_path_buf();
    assert_eq!(
        CtrlClient::connect(&path, &root.join("profile").join("ctrl.token"), T).unwrap_err().code(),
        "ctrl_unsafe_runtime_dir"
    );
    let s = client::status(&app_target(&root));
    assert!(!s.found && !s.authenticated);
    chmod(&root.join("run"), 0o700);
    client::connect(&app_target(&root)).unwrap().close();
    server.stop().unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_runtime_dir_is_unsafe() {
    let root = temp_dir("symdir");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    let link = root.join("run-link");
    std::os::unix::fs::symlink(root.join("run"), &link).unwrap();
    let target = Target { runtime_dir: link, ..app_target(&root) };
    assert_eq!(client::connect(&target).unwrap_err().code(), "ctrl_unsafe_runtime_dir");
    server.stop().unwrap();
}

#[cfg(unix)]
#[test]
fn group_writable_or_linked_discovery_file_is_unsafe() {
    let root = temp_dir("gwfile");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    let path = server.discovery_path().to_path_buf();
    chmod(&path, 0o620);
    let target = Target { instance_id: Some(server.instance_id().to_string()), ..app_target(&root) };
    assert_eq!(client::connect(&target).unwrap_err().code(), "ctrl_unsafe_runtime_dir");
    chmod(&path, 0o600);
    // A symlink to the real file under another instance name.
    let link = root.join("run").join(Discovery::file_name("linked"));
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let linked = Target { instance_id: Some("linked".into()), ..app_target(&root) };
    assert_eq!(client::connect(&linked).unwrap_err().code(), "ctrl_unsafe_runtime_dir");
    // Without an instance the unsafe link is skipped and the real file is used.
    client::connect(&app_target(&root)).unwrap().close();
    server.stop().unwrap();
}

#[cfg(unix)]
#[test]
fn unsafe_token_dir_is_token_permissions() {
    let root = temp_dir("tokdir");
    let ops = Arc::new(Ops::default());
    let server = start(&root, &ops);
    chmod(&root.join("profile"), 0o750);
    assert_eq!(client::connect(&app_target(&root)).unwrap_err(), ClientError::Token(AuthError::TokenPermissions));
    chmod(&root.join("profile"), 0o700);
    server.stop().unwrap();
}
