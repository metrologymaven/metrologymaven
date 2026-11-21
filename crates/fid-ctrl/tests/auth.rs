//! Acceptance criteria of spec node AUTO.CTRL.AUTH (AC-1 to AC-7) against an in-process
//! test app. Every listener binds port 0; every token file lives in its own temp directory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_ctrl::auth::{
    client_proof, server_proof, verify_client_proof, verify_server_proof, CLIENT_LABEL, SERVER_LABEL,
};
use fid_ctrl::frame::{Frame, LineReader};
use fid_ctrl::token::Permissions;
use fid_ctrl::{
    Config, Handler, Log, MemoryLog, Nonce, Proof, Reply, ReplyBody, Request, Server, StartError, Token, TokenFile,
};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Answers `commands.list` with a `commands` event and counts every request it sees.
#[derive(Default)]
struct Ops {
    seen: AtomicUsize,
}

impl Handler for Ops {
    fn handle(&self, req: &Request) -> ReplyBody {
        self.seen.fetch_add(1, Ordering::SeqCst);
        let mut o = BTreeMap::new();
        o.insert("kind".to_string(), CValue::Str("commands".into()));
        o.insert("op".to_string(), CValue::Str(req.op.to_string()));
        ReplyBody::Event(CValue::Obj(o))
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-ctrl-auth-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

struct App {
    server: Server,
    log: Arc<MemoryLog>,
    ops: Arc<Ops>,
    token: PathBuf,
}

fn app_with(name: &str, tune: impl FnOnce(&mut Config)) -> App {
    let root = temp_dir(name);
    let token = root.join("profile").join("ctrl.token");
    let mut cfg = Config::new("127.0.0.1", &root.join("run"), &token);
    tune(&mut cfg);
    let log = Arc::new(MemoryLog::default());
    let ops = Arc::new(Ops::default());
    let server = Server::start(cfg, ops.clone() as Arc<dyn Handler>, log.clone() as Arc<dyn Log>).unwrap();
    App { server, log, ops, token }
}

fn app(name: &str) -> App {
    app_with(name, |_| {})
}

fn token_bytes(path: &Path) -> [u8; 32] {
    std::fs::read(path).unwrap().try_into().unwrap()
}

/// Records every byte sent and received, for AC-7.
#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl Capture {
    fn push(&self, b: &[u8]) {
        self.0.lock().unwrap().extend_from_slice(b);
    }
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

struct Tap {
    stream: TcpStream,
    capture: Arc<Capture>,
}

impl Read for Tap {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.stream.read(buf)?;
        self.capture.push(&buf[..n]);
        Ok(n)
    }
}

struct Client {
    stream: TcpStream,
    reader: LineReader<Tap>,
    capture: Arc<Capture>,
}

impl Client {
    fn connect(app: &App) -> Client {
        Client::connect_capturing(app, Arc::new(Capture::default()))
    }

    fn connect_capturing(app: &App, capture: Arc<Capture>) -> Client {
        let stream = TcpStream::connect(app.server.local_addr()).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let tap = Tap { stream: stream.try_clone().unwrap(), capture: capture.clone() };
        Client { stream, reader: LineReader::new(tap), capture }
    }

    fn send(&mut self, line: &str) {
        self.capture.push(line.as_bytes());
        self.capture.push(b"\n");
        self.stream.write_all(line.as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
    }

    fn recv(&mut self, op: &str) -> Reply {
        match self.reader.next_line(1024 * 1024).unwrap() {
            Frame::Line(l) => Reply::decode(&l, op).unwrap(),
            other => panic!("expected a reply line, got {other:?}"),
        }
    }

    fn expect_closed(&mut self) {
        assert_eq!(self.reader.next_line(1024).unwrap(), Frame::Eof);
    }

    /// Sends `hello` and returns the server nonce.
    fn hello(&mut self) -> Nonce {
        self.send(r#"{"v":1,"id":1,"op":"hello","params":{"versions":[1]}}"#);
        let r = self.recv("hello");
        assert_eq!(str_field(event(&r), "kind"), "hello", "{r:?}");
        Nonce::from_hex(str_field(event(&r), "nonce")).unwrap()
    }

    /// Sends a correct `auth` for `server_nonce`; returns the line sent and the reply.
    fn auth(&mut self, token: &Token, server_nonce: &Nonce) -> (String, Nonce, Reply) {
        let client_nonce = Nonce::random().unwrap();
        let line = auth_line(&client_nonce, &client_proof(token, server_nonce, &client_nonce));
        self.send(&line);
        (line, client_nonce, self.recv("auth"))
    }
}

fn auth_line(nonce: &Nonce, proof: &Proof) -> String {
    format!(r#"{{"v":1,"id":2,"op":"auth","params":{{"nonce":"{}","proof":"{}"}}}}"#, nonce.to_hex(), proof.to_hex())
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
    assert_eq!(str_field(o, "kind"), "error", "{r:?}");
    str_field(o, "code").to_string()
}

fn wait_for_log(a: &App, code: &str) {
    let t = Instant::now();
    while !a.log.contains(code) {
        assert!(t.elapsed() < Duration::from_secs(10), "no {code} log");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Owner-only, read with PowerShell's `Get-Acl` (.NET security API), independently of the
/// crate's own check: a protected DACL whose only rules allow the current user's SID.
#[cfg(windows)]
fn assert_owner_only_by_os(path: &Path) {
    let script = format!(
        "$a = Get-Acl -LiteralPath '{}'; 'protected=' + $a.AreAccessRulesProtected; \
         foreach ($r in $a.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) \
         {{ 'rule=' + $r.AccessControlType + ' ' + $r.IdentityReference.Value }}; \
         'user=' + [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value",
        path.display()
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let user = lines.iter().find_map(|l| l.strip_prefix("user=")).unwrap();
    assert!(user.starts_with("S-1-"), "{text}");
    assert!(lines.contains(&"protected=True"), "{text}");
    let rules: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("rule=")).collect();
    assert!(!rules.is_empty(), "{text}");
    for r in rules {
        assert_eq!(r, format!("Allow {user}"), "{text}");
    }
}

/// Owner-only, read with `stat` (mode and owner) on the file.
#[cfg(unix)]
fn assert_owner_only_by_os(path: &Path) {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path).unwrap();
    assert_eq!(m.mode() & 0o777, 0o600, "{:o}", m.mode());
    assert!(m.is_file());
    // The test process's uid: the owner of a file it creates.
    let probe = path.with_file_name("uid-probe");
    std::fs::write(&probe, b"").unwrap();
    let me = std::fs::metadata(&probe).unwrap().uid();
    std::fs::remove_file(&probe).unwrap();
    assert_eq!(m.uid(), me);
}

/// Widens the token file: group-readable (Unix) or an Everyone read ACE (Windows).
fn widen(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("icacls").arg(path).arg("/grant").arg("*S-1-1-0:(R)").output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    }
}

#[test]
fn ac1_fresh_profile_gets_a_32_byte_owner_only_token_file() {
    let a = app("ac1");
    assert!(a.token.parent().unwrap().is_dir(), "the profile directory is created");
    let bytes = std::fs::read(&a.token).unwrap();
    assert_eq!(bytes.len(), 32);
    assert_ne!(bytes, vec![0u8; 32]);
    assert_owner_only_by_os(&a.token);
    let tf = TokenFile::ensure(&a.token).unwrap();
    let f = std::fs::File::open(&a.token).unwrap();
    assert_eq!(tf.permissions(&f).unwrap(), Permissions::OwnerOnly);
    // An existing token is kept, not regenerated, and no temporary file is left behind.
    a.server.stop().unwrap();
    TokenFile::ensure(&a.token).unwrap();
    assert_eq!(std::fs::read(&a.token).unwrap(), bytes);
    let names: Vec<_> = std::fs::read_dir(a.token.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, vec![std::ffi::OsString::from("ctrl.token")]);
}

#[test]
fn ac1_two_fresh_profiles_get_different_tokens() {
    let a = app("ac1-a");
    let b = app("ac1-b");
    assert_ne!(token_bytes(&a.token), token_bytes(&b.token));
}

#[test]
fn ac2_wide_token_file_refuses_a_correct_client_until_recreated() {
    let a = app("ac2");
    widen(&a.token);
    let tf = TokenFile::ensure(&a.token).unwrap();
    let f = std::fs::File::open(&a.token).unwrap();
    assert!(matches!(tf.permissions(&f).unwrap(), Permissions::Wider(_)));
    drop(f);
    let token = Token::from_bytes(token_bytes(&a.token));
    for _ in 0..2 {
        let mut c = Client::connect(&a);
        let sn = c.hello();
        let (_, _, r) = c.auth(&token, &sn);
        assert_eq!(error_code(&r), "token_permissions");
        c.expect_closed();
    }
    assert!(a.log.contains("token_permissions"));
    assert_eq!(a.ops.seen.load(Ordering::SeqCst), 0);
    // Recreated (as the next app start does when the file is missing): accepted again.
    std::fs::remove_file(&a.token).unwrap();
    TokenFile::ensure(&a.token).unwrap();
    let token = Token::from_bytes(token_bytes(&a.token));
    let mut c = Client::connect(&a);
    let sn = c.hello();
    let (_, _, r) = c.auth(&token, &sn);
    assert_eq!(str_field(event(&r), "kind"), "auth", "{r:?}");
}

#[test]
fn ac2_app_started_on_a_wide_token_file_logs_and_refuses() {
    let root = temp_dir("ac2-start");
    let token = root.join("ctrl.token");
    TokenFile::ensure(&token).unwrap();
    widen(&token);
    let log = Arc::new(MemoryLog::default());
    let cfg = Config::new("127.0.0.1", &root.join("run"), &token);
    let server = Server::start(cfg, Arc::new(Ops::default()), log.clone() as Arc<dyn Log>).unwrap();
    assert!(log.contains("token_permissions"));
    let a = App { server, log, ops: Arc::new(Ops::default()), token };
    let t = Token::from_bytes(token_bytes(&a.token));
    let mut c = Client::connect(&a);
    let sn = c.hello();
    assert_eq!(error_code(&c.auth(&t, &sn).2), "token_permissions");
    c.expect_closed();
}

#[cfg(unix)]
#[test]
fn ac2_other_and_group_write_bits_are_refused_too() {
    use std::os::unix::fs::PermissionsExt;
    for mode in [0o604, 0o620, 0o602, 0o660, 0o700, 0o400] {
        let root = temp_dir(&format!("mode-{mode:o}"));
        let path = root.join("ctrl.token");
        let tf = TokenFile::ensure(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        let f = std::fs::File::open(&path).unwrap();
        let wide = mode & 0o077 != 0;
        assert_eq!(matches!(tf.permissions(&f).unwrap(), Permissions::Wider(_)), wide, "{mode:o}");
    }
}

#[test]
fn ac3_correct_client_verifies_both_proofs_and_lists_commands() {
    let a = app("ac3");
    let token = Token::from_bytes(token_bytes(&a.token));
    let mut c = Client::connect(&a);
    let sn = c.hello();
    let (_, cn, r) = c.auth(&token, &sn);
    assert_eq!(r.id, Some(2));
    let o = event(&r);
    assert_eq!(str_field(o, "kind"), "auth");
    let server = Proof::from_hex(str_field(o, "proof")).unwrap();
    assert!(verify_server_proof(&token, &cn, &sn, &server));
    assert_eq!(server, server_proof(&token, &cn, &sn));
    c.send(r#"{"v":1,"id":3,"op":"commands.list","params":{}}"#);
    let r = c.recv("commands.list");
    assert_eq!(r.id, Some(3));
    assert_eq!(str_field(event(&r), "kind"), "commands");
    assert_eq!(a.ops.seen.load(Ordering::SeqCst), 1);
    assert!(a.log.contains("ctrl_authenticated"));
}

#[test]
fn ac3_client_detects_a_wrong_server_proof() {
    let token = Token::from_bytes([7u8; 32]);
    let other = Token::from_bytes([8u8; 32]);
    let (sn, cn) = (Nonce([1u8; 32]), Nonce([2u8; 32]));
    let good = server_proof(&token, &cn, &sn);
    assert!(verify_server_proof(&token, &cn, &sn, &good));
    assert!(!verify_server_proof(&token, &sn, &cn, &good), "nonce order matters");
    assert!(!verify_server_proof(&other, &cn, &sn, &good), "another token");
    // A client proof is not a server proof (distinct labels).
    assert!(!verify_server_proof(&token, &cn, &sn, &client_proof(&token, &cn, &sn)));
    let mut flipped = good;
    flipped.0[31] ^= 1;
    assert!(!verify_server_proof(&token, &cn, &sn, &flipped));
}

#[test]
fn ac4_recorded_auth_from_a_previous_connection_is_refused() {
    let a = app("ac4");
    let token = Token::from_bytes(token_bytes(&a.token));
    let mut first = Client::connect(&a);
    let sn = first.hello();
    let (recorded, _, r) = first.auth(&token, &sn);
    assert_eq!(str_field(event(&r), "kind"), "auth");
    let mut second = Client::connect(&a);
    let sn2 = second.hello();
    assert_ne!(sn, sn2, "every hello carries a fresh nonce");
    second.send(&recorded);
    let r = second.recv("auth");
    assert_eq!(error_code(&r), "auth_failed");
    second.expect_closed();
    // Replaying it on the connection that already used it is refused as well.
    first.send(&recorded);
    assert_eq!(error_code(&first.recv("auth")), "auth_failed");
    first.expect_closed();
}

#[test]
fn ac4_a_server_nonce_allows_one_attempt_only() {
    let a = app("ac4-once");
    let token = Token::from_bytes(token_bytes(&a.token));
    let mut c = Client::connect(&a);
    let sn = c.hello();
    let cn = Nonce::random().unwrap();
    c.send(&auth_line(&cn, &Proof([0u8; 32])));
    assert_eq!(error_code(&c.recv("auth")), "auth_failed");
    c.expect_closed();
    // A correct proof for that (spent) server nonce on a new connection: its hello has a new nonce.
    let mut c = Client::connect(&a);
    let _fresh = c.hello();
    c.send(&auth_line(&cn, &client_proof(&token, &sn, &cn)));
    assert_eq!(error_code(&c.recv("auth")), "auth_failed");
    c.expect_closed();
}

#[test]
fn ac5_operations_before_auth_are_refused_and_closed() {
    let a = app("ac5");
    let execute = r#"{"v":1,"id":9,"op":"command.execute","params":{"command":"project.save@1"}}"#;
    // Before hello.
    let mut c = Client::connect(&a);
    c.send(execute);
    let r = c.recv("command.execute");
    assert_eq!(error_code(&r), "auth_failed");
    assert_eq!(r.id, Some(9));
    c.expect_closed();
    // After hello, before auth.
    let mut c = Client::connect(&a);
    c.hello();
    c.send(execute);
    assert_eq!(error_code(&c.recv("command.execute")), "auth_failed");
    c.expect_closed();
    assert_eq!(a.ops.seen.load(Ordering::SeqCst), 0, "no operation reached the handler");
    assert!(a.log.contains("auth_failed"));
}

#[test]
fn ac5_wrong_or_malformed_auth_is_refused_and_closed() {
    let a = app("ac5-proof");
    let wrong = Token::from_bytes([0x5a; 32]);
    let token = Token::from_bytes(token_bytes(&a.token));
    // Wrong token.
    let mut c = Client::connect(&a);
    let sn = c.hello();
    assert_eq!(error_code(&c.auth(&wrong, &sn).2), "auth_failed");
    c.expect_closed();
    // auth without hello, even with a proof for some nonce.
    let mut c = Client::connect(&a);
    let cn = Nonce::random().unwrap();
    c.send(&auth_line(&cn, &client_proof(&token, &Nonce([0; 32]), &cn)));
    assert_eq!(error_code(&c.recv("auth")), "auth_failed");
    c.expect_closed();
    // Malformed params.
    for params in [
        r#"{}"#,
        r#"{"nonce":"00"}"#,
        r#"{"nonce":"zz","proof":"00"}"#,
        r#"{"nonce":1,"proof":2}"#,
        r#"{"nonce":"0000000000000000000000000000000000000000000000000000000000000000","proof":"0000000000000000000000000000000000000000000000000000000000000000","extra":1}"#,
        r#"{"nonce":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","proof":"0000000000000000000000000000000000000000000000000000000000000000"}"#,
    ] {
        let mut c = Client::connect(&a);
        c.hello();
        c.send(&format!(r#"{{"v":1,"id":5,"op":"auth","params":{params}}}"#));
        let r = c.recv("auth");
        assert_eq!(error_code(&r), "auth_failed", "{params}");
        assert_eq!(r.id, Some(5));
        c.expect_closed();
    }
    assert_eq!(a.ops.seen.load(Ordering::SeqCst), 0);
}

#[test]
fn authentication_must_finish_within_the_first_request_deadline() {
    let a = app_with("deadline", |c| c.first_request_timeout = Duration::from_millis(300));
    let t = Instant::now();
    let mut c = Client::connect(&a);
    c.hello();
    // hello alone does not extend the deadline to the 300 s idle timeout.
    assert_eq!(error_code(&c.recv("hello")), "read_timeout");
    c.expect_closed();
    assert!(t.elapsed() < Duration::from_secs(30));
    // After authenticating, the idle timeout applies instead.
    let token = Token::from_bytes(token_bytes(&a.token));
    let mut c = Client::connect(&a);
    let sn = c.hello();
    assert_eq!(str_field(event(&c.auth(&token, &sn).2), "kind"), "auth");
    std::thread::sleep(Duration::from_millis(600));
    c.send(r#"{"v":1,"id":3,"op":"commands.list","params":{}}"#);
    assert_eq!(str_field(event(&c.recv("commands.list")), "kind"), "commands");
}

#[test]
fn token_file_of_the_wrong_length_is_token_io() {
    let a = app("length");
    for len in [0usize, 31, 33] {
        let tf = TokenFile::ensure(&a.token).unwrap();
        // Rewrite in place so the owner-only permissions stay.
        let mut f = std::fs::OpenOptions::new().write(true).truncate(true).open(&a.token).unwrap();
        f.write_all(&vec![1u8; len]).unwrap();
        drop(f);
        assert_eq!(tf.load().err(), Some(fid_ctrl::AuthError::TokenIo), "{len}");
        let mut c = Client::connect(&a);
        let sn = c.hello();
        assert_eq!(error_code(&c.auth(&Token::from_bytes([1u8; 32]), &sn).2), "token_io", "{len}");
        c.expect_closed();
    }
    // A missing token file is token_io too.
    std::fs::remove_file(&a.token).unwrap();
    let mut c = Client::connect(&a);
    let sn = c.hello();
    assert_eq!(error_code(&c.auth(&Token::from_bytes([1u8; 32]), &sn).2), "token_io");
}

#[test]
fn hello_nonces_are_fresh_hex_and_absent_after_auth() {
    let a = app("nonces");
    let token = Token::from_bytes(token_bytes(&a.token));
    let mut c = Client::connect(&a);
    let n1 = c.hello();
    let n2 = c.hello();
    assert_ne!(n1, n2);
    // The latest hello's nonce is the one that counts.
    let (_, _, r) = c.auth(&token, &n2);
    assert_eq!(str_field(event(&r), "kind"), "auth");
    c.send(r#"{"v":1,"id":1,"op":"hello","params":{"versions":[1]}}"#);
    let r = c.recv("hello");
    assert!(!event(&r).contains_key("nonce"), "{r:?}");
}

#[test]
fn ac6_proofs_reproduce_the_python_reference_vectors() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ctrl/auth-vectors.json");
    let CValue::Obj(doc) = parse(&std::fs::read(path).unwrap()).unwrap() else { panic!("not an object") };
    assert_eq!(str_field(&doc, "client_label").as_bytes(), CLIENT_LABEL);
    assert_eq!(str_field(&doc, "server_label").as_bytes(), SERVER_LABEL);
    let Some(CValue::Arr(cases)) = doc.get("cases") else { panic!("no cases") };
    assert!(cases.len() >= 5);
    let hex32 = |s: &str| Nonce::from_hex(s).unwrap().0;
    for case in cases {
        let CValue::Obj(c) = case else { panic!("case") };
        let name = str_field(c, "name");
        let token = Token::from_bytes(hex32(str_field(c, "token")));
        let sn = Nonce::from_hex(str_field(c, "server_nonce")).unwrap();
        let cn = Nonce::from_hex(str_field(c, "client_nonce")).unwrap();
        let cp = client_proof(&token, &sn, &cn);
        let sp = server_proof(&token, &cn, &sn);
        assert_eq!(cp.to_hex(), str_field(c, "client_proof"), "{name}");
        assert_eq!(sp.to_hex(), str_field(c, "server_proof"), "{name}");
        assert!(verify_client_proof(&token, &sn, &cn, &cp), "{name}");
        assert!(verify_server_proof(&token, &cn, &sn, &sp), "{name}");
        for bit in [0usize, 7, 128, 255] {
            let mut bad = cp;
            bad.0[bit / 8] ^= 1 << (bit % 8);
            assert!(!verify_client_proof(&token, &sn, &cn, &bad), "{name} bit {bit}");
        }
    }
}

fn base64(b: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut s = String::new();
    for chunk in b.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, &x)| acc | (u32::from(x) << (16 - 8 * i)));
        let chars = chunk.len() + 1;
        for i in 0..4 {
            if i < chars {
                s.push(char::from(alphabet[((n >> (18 - 6 * i)) & 63) as usize]));
            } else if pad {
                s.push('=');
            }
        }
    }
    s
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn ac7_token_bytes_hex_and_base64_appear_in_no_message_or_log() {
    let a = app("ac7");
    let raw = token_bytes(&a.token);
    let token = Token::from_bytes(raw);
    let capture = Arc::new(Capture::default());
    // A successful session, a failed proof, an operation before auth, a widened file.
    let mut c = Client::connect_capturing(&a, capture.clone());
    let sn = c.hello();
    c.auth(&token, &sn);
    c.send(r#"{"v":1,"id":3,"op":"commands.list","params":{}}"#);
    c.recv("commands.list");
    let mut c = Client::connect_capturing(&a, capture.clone());
    let sn = c.hello();
    c.auth(&Token::from_bytes([3u8; 32]), &sn);
    let mut c = Client::connect_capturing(&a, capture.clone());
    c.send(r#"{"v":1,"id":4,"op":"commands.list","params":{}}"#);
    c.recv("commands.list");
    widen(&a.token);
    let mut c = Client::connect_capturing(&a, capture.clone());
    let sn = c.hello();
    c.auth(&token, &sn);
    wait_for_log(&a, "token_permissions");
    // Errors that mention the token file.
    let start_err = StartError::Token(std::io::Error::other("x")).to_string();
    let mut text = capture.bytes();
    for (code, detail) in a.log.entries() {
        text.extend_from_slice(code.as_bytes());
        text.push(b' ');
        text.extend_from_slice(detail.as_bytes());
        text.push(b'\n');
    }
    text.extend_from_slice(start_err.as_bytes());
    for e in [fid_ctrl::AuthError::TokenIo, fid_ctrl::AuthError::TokenPermissions, fid_ctrl::AuthError::AuthFailed] {
        text.extend_from_slice(e.to_string().as_bytes());
    }
    assert!(text.len() > 500, "the capture holds the session");
    const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let lower: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let forms: Vec<Vec<u8>> = vec![
        raw.to_vec(),
        lower.clone().into_bytes(),
        lower.to_ascii_uppercase().into_bytes(),
        base64(&raw, STD, true).into_bytes(),
        base64(&raw, STD, false).into_bytes(),
        base64(&raw, URL, false).into_bytes(),
    ];
    for f in &forms {
        assert!(!contains(&text, f), "token form {:?} leaked", String::from_utf8_lossy(f));
    }
    // A prefix check, so a truncated echo would be caught too.
    assert!(!contains(&text, &lower.as_bytes()[..16]));
}
