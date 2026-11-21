//! Acceptance criteria of spec node AUTO.CTRL.WIRE (AC-1, AC-2, AC-3, AC-5) and the
//! requirements behind them, against an in-process test app. Every listener binds port 0.
//! Operations need an authenticated connection (AUTO.CTRL.AUTH); [`Client::login`] runs the
//! handshake. The authentication acceptance criteria are in `tests/auth.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_ctrl::auth::{client_proof, verify_server_proof};
use fid_ctrl::frame::{Frame, LineReader};
use fid_ctrl::msg::MAX_SCREENSHOT_REPLY_BYTES;
use fid_ctrl::{
    Config, Discovery, Handler, Log, MemoryLog, Nonce, Proof, Reply, ReplyBody, Request, Server, StartError, Token,
};
use fid_surface::Envelope;
use std::collections::BTreeMap;
use std::io::Write;
use std::net::{Shutdown, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Answers `ping` with a `pong` event and `env.test` with a computed envelope.
struct TestOps;

impl Handler for TestOps {
    fn handle(&self, req: &Request) -> ReplyBody {
        if req.op.as_str() == "env.test" {
            return ReplyBody::Envelope(Box::new(Envelope::computed("ctrl.ping@1", CValue::Bool(true))));
        }
        let mut o = BTreeMap::new();
        o.insert("kind".to_string(), CValue::Str("pong".into()));
        o.insert("op".to_string(), CValue::Str(req.op.to_string()));
        ReplyBody::Event(CValue::Obj(o))
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-ctrl-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// The token file of a test app: outside the discovery directory, so that directory holds
/// only the discovery file.
fn token_path(name: &str) -> PathBuf {
    temp_dir(&format!("{name}-token")).join("ctrl.token")
}

struct App {
    server: Server,
    log: Arc<MemoryLog>,
    dir: PathBuf,
    token: PathBuf,
}

fn app_with(name: &str, tune: impl FnOnce(&mut Config)) -> App {
    let dir = temp_dir(name);
    let token = token_path(name);
    let mut cfg = Config::new("127.0.0.1", &dir, &token);
    tune(&mut cfg);
    let log = Arc::new(MemoryLog::default());
    let server = Server::start(cfg, Arc::new(TestOps), log.clone() as Arc<dyn Log>).unwrap();
    App { server, log, dir, token }
}

fn app(name: &str) -> App {
    app_with(name, |_| {})
}

struct Client {
    stream: TcpStream,
    reader: LineReader<TcpStream>,
}

impl Client {
    fn connect(app: &App) -> Client {
        let stream = TcpStream::connect(app.server.local_addr()).unwrap();
        // Fail-safe only: a hung test fails instead of blocking CI.
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let reader = LineReader::new(stream.try_clone().unwrap());
        Client { stream, reader }
    }

    fn send(&mut self, line: &str) {
        self.stream.write_all(line.as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
    }

    fn recv(&mut self, op: &str) -> Reply {
        match self.reader.next_line(MAX_SCREENSHOT_REPLY_BYTES).unwrap() {
            Frame::Line(l) => Reply::decode(&l, op).unwrap(),
            other => panic!("expected a reply line, got {other:?}"),
        }
    }

    fn expect_closed(&mut self) {
        assert_eq!(self.reader.next_line(1024).unwrap(), Frame::Eof);
    }

    fn hello(&mut self) -> Reply {
        self.send(r#"{"v":1,"id":1,"op":"hello","params":{"versions":[1]}}"#);
        self.recv("hello")
    }

    /// `hello` then `auth` with the app's token; both proofs must verify.
    fn login(&mut self, app: &App) {
        let r = self.hello();
        assert_hello_ok(&r);
        let server_nonce = Nonce::from_hex(str_field(event(&r), "nonce")).unwrap();
        // The client checks the instance against the discovery file before authenticating.
        let discovered = Discovery::decode(&std::fs::read(app.server.discovery_path()).unwrap()).unwrap();
        let instance = str_field(event(&r), "instance_id").to_string();
        assert_eq!(instance, discovered.instance_id);
        let token = Token::from_bytes(std::fs::read(&app.token).unwrap().try_into().unwrap());
        let client_nonce = Nonce::random().unwrap();
        let proof = client_proof(&token, &instance, &server_nonce, &client_nonce);
        self.send(&format!(
            r#"{{"v":1,"id":2,"op":"auth","params":{{"nonce":"{}","proof":"{}"}}}}"#,
            client_nonce.to_hex(),
            proof.to_hex()
        ));
        let r = self.recv("auth");
        assert_eq!(str_field(event(&r), "kind"), "auth", "{r:?}");
        let server = Proof::from_hex(str_field(event(&r), "proof")).unwrap();
        assert!(verify_server_proof(&token, &instance, &client_nonce, &server_nonce, &server));
    }
}

fn event(r: &Reply) -> &BTreeMap<String, CValue> {
    match &r.body {
        ReplyBody::Event(CValue::Obj(o)) => o,
        other => panic!("expected an event, got {other:?}"),
    }
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: {other:?}"),
    }
}

fn error_code(r: &Reply) -> String {
    let o = event(r);
    assert_eq!(str_field(o, "kind"), "error");
    str_field(o, "code").to_string()
}

fn assert_hello_ok(r: &Reply) {
    let o = event(r);
    assert_eq!(str_field(o, "kind"), "hello", "{r:?}");
    assert_eq!(o.get("version"), Some(&CValue::Num(1.0)));
}

#[test]
fn ac1_non_loopback_address_binds_nothing_and_logs() {
    for addr in ["0.0.0.0", "::", "192.168.1.10", "127.0.0.2", "localhost", "", "127.0.0.1:80", "::ffff:127.0.0.1"] {
        let dir = temp_dir("ac1");
        let token = token_path("ac1");
        let log = Arc::new(MemoryLog::default());
        let r = Server::start(Config::new(addr, &dir, &token), Arc::new(TestOps), log.clone() as Arc<dyn Log>);
        match r {
            Err(e @ StartError::NonLoopback(_)) => assert_eq!(e.code(), "ctrl_non_loopback"),
            Err(e) => panic!("{addr}: wrong error {e}"),
            Ok(s) => panic!("{addr}: listener started on {}", s.local_addr()),
        }
        assert!(log.contains("ctrl_non_loopback"), "{addr}");
        assert!(!log.contains("ctrl_listening"), "{addr}");
        // No discovery file (the directory is not even created), and no token file.
        assert!(!dir.exists(), "{addr}");
        assert!(!token.exists(), "{addr}");
    }
}

#[test]
fn ac2_discovery_file_has_port_no_secret_owner_only_and_is_removed() {
    let a = app("ac2");
    let path = a.server.discovery_path().to_path_buf();
    assert!(path.starts_with(&a.dir));
    let bytes = std::fs::read(&path).unwrap();
    let d = Discovery::decode(&bytes).unwrap();
    assert_eq!(d.port, a.server.local_addr().port());
    assert_ne!(d.port, 0);
    assert_eq!(d.pid, std::process::id());
    assert_eq!(d.instance_id, a.server.instance_id());
    assert_eq!(d.versions, vec![1]);
    let CValue::Obj(o) = parse(&bytes).unwrap() else { panic!("not an object") };
    for k in o.keys() {
        let k = k.to_ascii_lowercase();
        assert!(!k.contains("token") && !k.contains("secret"), "{k}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    }
    // A client can find the app from the file alone.
    let probe = TcpStream::connect((d.address, d.port)).unwrap();
    drop(probe);
    a.server.stop().unwrap();
    assert!(!path.exists());
    // No temporary file is left behind either.
    assert_eq!(std::fs::read_dir(&a.dir).unwrap().count(), 0);
}

#[test]
fn ac3_two_mib_line_is_rejected_and_server_keeps_working() {
    let a = app("ac3");
    let mut c = Client::connect(&a);
    let big = format!(r#"{{"v":1,"id":5,"op":"ping","params":{{"pad":"{}"}}}}"#, "x".repeat(2 * 1024 * 1024));
    c.send(&big);
    let r = c.recv("ping");
    assert_eq!(error_code(&r), "message_too_large");
    assert_eq!(r.id, None);
    c.expect_closed();
    assert!(a.log.contains("message_too_large"));
    let mut c2 = Client::connect(&a);
    c2.login(&a);
    c2.send(r#"{"v":1,"id":3,"op":"ping","params":{}}"#);
    assert_eq!(str_field(event(&c2.recv("ping")), "kind"), "pong");
}

#[test]
fn line_of_exactly_one_mib_is_accepted_and_one_more_byte_is_not() {
    let a = app("bound");
    let head = r#"{"v":1,"id":9,"op":"ping","params":{"pad":""#;
    let tail = r#""}}"#;
    let fill = 1024 * 1024 - head.len() - tail.len();
    let mut c = Client::connect(&a);
    c.login(&a);
    c.send(&format!("{head}{}{tail}", "x".repeat(fill)));
    let r = c.recv("ping");
    assert_eq!(r.id, Some(9));
    assert_eq!(str_field(event(&r), "kind"), "pong");
    c.send(&format!("{head}{}{tail}", "x".repeat(fill + 1)));
    assert_eq!(error_code(&c.recv("ping")), "message_too_large");
    c.expect_closed();
}

#[test]
fn ac5_hello_with_unknown_versions_is_refused_listing_supported() {
    let a = app("ac5");
    let mut c = Client::connect(&a);
    c.send(r#"{"v":1,"id":3,"op":"hello","params":{"versions":[99]}}"#);
    let r = c.recv("hello");
    assert_eq!(r.id, Some(3));
    assert_eq!(error_code(&r), "version_unsupported");
    assert_eq!(event(&r).get("versions"), Some(&CValue::Arr(vec![CValue::Num(1.0)])));
    c.expect_closed();
}

#[test]
fn hello_negotiates_and_operations_get_replies() {
    let a = app("hello");
    let mut c = Client::connect(&a);
    c.send(r#"{"v":1,"id":1,"op":"hello","params":{"versions":[7,1,2]}}"#);
    let r = c.recv("hello");
    assert_hello_ok(&r);
    assert_eq!(str_field(event(&r), "instance_id"), a.server.instance_id());
    let mut c = Client::connect(&a);
    c.login(&a);
    c.send(r#"{"v":1,"id":42,"op":"env.test","params":{}}"#);
    let r = c.recv("env.test");
    assert_eq!(r.id, Some(42));
    match r.body {
        ReplyBody::Envelope(e) => assert_eq!(e.command_id, "ctrl.ping@1"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn malformed_lines_get_typed_replies_and_the_connection_survives() {
    let a = app("malformed");
    let mut c = Client::connect(&a);
    c.login(&a);
    for (line, id) in [
        ("not json", None),
        ("", None),
        ("[1,2]", None),
        (r#"{"v":1,"id":7,"op":"Bad Op","params":{}}"#, Some(7)),
        (r#"{"v":1,"id":8,"op":"ping"}"#, Some(8)),
        (r#"{"v":1,"id":9,"op":"ping","params":{},"extra":1}"#, Some(9)),
        (r#"{"v":1,"id":-1,"op":"ping","params":{}}"#, None),
        (r#"{"v":1,"id":10,"op":"hello","params":{"versions":"1"}}"#, Some(10)),
        (r#"{"v":1,"id":11,"op":"ping","params":{},"params":{}}"#, None),
    ] {
        c.send(line);
        let r = c.recv("ping");
        assert_eq!(error_code(&r), "malformed_message", "{line}");
        assert_eq!(r.id, id, "{line}");
    }
    c.stream.write_all(b"\xff\xfe{}\n").unwrap();
    assert_eq!(error_code(&c.recv("ping")), "malformed_message");
    c.send(r#"{"v":1,"id":12,"op":"ping","params":{}}"#);
    assert_eq!(c.recv("ping").id, Some(12));
}

#[test]
fn unsupported_request_version_is_refused_and_closed() {
    let a = app("reqv");
    let mut c = Client::connect(&a);
    c.send(r#"{"v":2,"id":4,"op":"ping","params":{}}"#);
    let r = c.recv("ping");
    assert_eq!(error_code(&r), "version_unsupported");
    assert_eq!(r.id, Some(4));
    c.expect_closed();
}

#[test]
fn ninth_connection_is_refused_and_a_slot_frees_on_disconnect() {
    let a = app("limit");
    let mut clients: Vec<Client> = (0..8).map(|_| Client::connect(&a)).collect();
    for c in &mut clients {
        assert_hello_ok(&c.hello());
    }
    assert_eq!(a.server.active_connections(), 8);
    // The refused client has already sent its hello; it still gets the typed reply
    // (no reset from closing with unread data).
    let mut ninth = Client::connect(&a);
    ninth.send(r#"{"v":1,"id":1,"op":"hello","params":{"versions":[1]}}"#);
    assert_eq!(error_code(&ninth.recv("hello")), "too_many_connections");
    ninth.expect_closed();
    let mut tenth = Client::connect(&a);
    assert_eq!(error_code(&tenth.recv("hello")), "too_many_connections");
    tenth.expect_closed();
    assert!(a.log.contains("too_many_connections"));
    let gone = clients.pop().unwrap();
    gone.stream.shutdown(Shutdown::Both).unwrap();
    drop(gone);
    let t = Instant::now();
    while a.server.active_connections() >= 8 {
        assert!(t.elapsed() < Duration::from_secs(10), "slot never freed");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_hello_ok(&Client::connect(&a).hello());
}

#[test]
fn stalled_partial_line_is_cut_off_at_the_line_deadline() {
    let a = app_with("slowloris", |c| c.line_timeout = Duration::from_millis(300));
    let mut c = Client::connect(&a);
    let t = Instant::now();
    c.stream.write_all(br#"{"v":1,"id":1,"op":"pi"#).unwrap();
    // Trickle one byte at a time: the deadline counts from the first byte, not the last.
    for b in br#"ng","params":{}"# {
        if c.stream.write_all(&[*b]).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let r = c.recv("ping");
    assert_eq!(error_code(&r), "read_timeout");
    assert!(t.elapsed() >= Duration::from_millis(300));
    assert!(a.log.contains("read_timeout"));
}

#[test]
fn idle_connection_is_closed_at_the_idle_deadline() {
    let a = app_with("idle", |c| c.idle_timeout = Duration::from_millis(200));
    let mut c = Client::connect(&a);
    assert_hello_ok(&c.hello());
    assert_eq!(error_code(&c.recv("ping")), "read_timeout");
    c.expect_closed();
}

#[test]
fn silent_new_connection_is_closed_at_the_first_request_deadline() {
    let a = app_with("first", |c| c.first_request_timeout = Duration::from_millis(200));
    let t = Instant::now();
    let mut c = Client::connect(&a);
    assert_eq!(error_code(&c.recv("ping")), "read_timeout");
    c.expect_closed();
    // Well under the 300 s idle timeout.
    assert!(t.elapsed() < Duration::from_secs(30));
    assert_eq!(Config::new("127.0.0.1", &a.dir, &a.token).first_request_timeout, Duration::from_secs(10));
}

/// Every logged detail and reply message stays one short line whatever the peer sent.
fn assert_echo_is_bounded(a: &App, r: &Reply) {
    let msg = str_field(event(r), "message");
    assert!(msg.len() < 200 && !msg.contains('\n'), "{msg:?}");
    for (code, detail) in a.log.entries() {
        assert!(detail.len() < 200 && !detail.contains('\n') && !detail.contains('\r'), "{code}: {detail:?}");
    }
}

#[test]
fn hostile_unknown_keys_are_escaped_and_truncated() {
    let a = app("keys");
    let mut c = Client::connect(&a);
    c.login(&a);
    c.send(r#"{"v":1,"id":21,"op":"ping","params":{},"a\nfake_log_line\r":1}"#);
    let r = c.recv("ping");
    assert_eq!(error_code(&r), "malformed_message");
    assert_eq!(r.id, Some(21));
    assert!(str_field(event(&r), "message").contains("\\n"));
    assert_echo_is_bounded(&a, &r);
    let huge = "k".repeat(900 * 1024);
    c.send(&format!(r#"{{"v":1,"id":22,"op":"ping","params":{{}},"{huge}":1}}"#));
    let r = c.recv("ping");
    assert_eq!(error_code(&r), "malformed_message");
    assert_eq!(r.id, Some(22));
    assert!(str_field(event(&r), "message").contains("[truncated]"));
    assert_echo_is_bounded(&a, &r);
    c.send(&format!(r#"{{"v":1,"id":23,"op":"hello","params":{{"versions":[1],"\n{huge}":1}}}}"#));
    let r = c.recv("hello");
    assert_eq!(error_code(&r), "malformed_message");
    assert_eq!(r.id, Some(23));
    assert_echo_is_bounded(&a, &r);
    // The connection still works.
    c.send(r#"{"v":1,"id":24,"op":"ping","params":{}}"#);
    assert_eq!(c.recv("ping").id, Some(24));
}

#[cfg(unix)]
#[test]
fn shared_or_symlinked_runtime_directory_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let token = token_path("unsafe-dir");
    let start = |dir: &std::path::Path| {
        let cfg = Config::new("127.0.0.1", dir, &token);
        Server::start(cfg, Arc::new(TestOps), Arc::new(MemoryLog::default()) as Arc<dyn Log>)
    };
    let refused = |r: Result<Server, StartError>| match r {
        Err(StartError::Io(e)) => assert!(e.to_string().contains("ctrl_unsafe_runtime_dir"), "{e}"),
        Err(e) => panic!("wrong error {e}"),
        Ok(s) => panic!("started with discovery file {}", s.discovery_path().display()),
    };
    let wide = temp_dir("wide");
    std::fs::create_dir_all(&wide).unwrap();
    std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o755)).unwrap();
    refused(start(&wide));
    assert_eq!(std::fs::read_dir(&wide).unwrap().count(), 0, "nothing left behind");
    let private = temp_dir("private");
    std::fs::create_dir_all(&private).unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let link = temp_dir("link");
    std::os::unix::fs::symlink(&private, &link).unwrap();
    refused(start(&link));
    assert_eq!(std::fs::read_dir(&private).unwrap().count(), 0, "nothing left behind");
    // The private directory itself is accepted.
    start(&private).unwrap().stop().unwrap();
}

#[test]
fn partial_frame_at_eof_is_dropped_without_reply() {
    let a = app("partial");
    let mut c = Client::connect(&a);
    c.stream.write_all(br#"{"v":1,"id":1,"op":"ping""#).unwrap();
    c.stream.shutdown(Shutdown::Write).unwrap();
    c.expect_closed();
    let t = Instant::now();
    while !a.log.contains("ctrl_partial_frame") {
        assert!(t.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn pipelined_requests_are_answered_in_order() {
    let a = app("pipeline");
    let mut c = Client::connect(&a);
    c.login(&a);
    let mut batch = String::new();
    for id in 1..=50 {
        batch.push_str(&format!("{{\"v\":1,\"id\":{id},\"op\":\"ping\",\"params\":{{}}}}\n"));
    }
    c.stream.write_all(batch.as_bytes()).unwrap();
    for id in 1..=50 {
        assert_eq!(c.recv("ping").id, Some(id));
    }
}

#[test]
fn stop_closes_open_connections() {
    let a = app("stop");
    let mut c = Client::connect(&a);
    assert_hello_ok(&c.hello());
    let path = a.server.discovery_path().to_path_buf();
    a.server.stop().unwrap();
    c.expect_closed();
    assert!(!path.exists());
    assert!(a.log.contains("ctrl_stopped"));
}

#[test]
fn ipv6_loopback_is_accepted_where_the_host_has_it() {
    let dir = temp_dir("v6");
    let token = token_path("v6");
    let log = Arc::new(MemoryLog::default());
    match Server::start(Config::new("::1", &dir, &token), Arc::new(TestOps), log as Arc<dyn Log>) {
        Ok(s) => {
            assert!(s.local_addr().ip().is_loopback() && s.local_addr().is_ipv6());
            let d = Discovery::decode(&std::fs::read(s.discovery_path()).unwrap()).unwrap();
            assert_eq!(d.address.to_string(), "::1");
            s.stop().unwrap();
        }
        // Some CI containers have no IPv6 loopback; refusal is the only other acceptable outcome.
        Err(StartError::Io(e)) => eprintln!("skipped: ::1 unavailable here ({e})"),
        Err(e) => panic!("{e}"),
    }
}
