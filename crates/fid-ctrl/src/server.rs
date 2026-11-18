//! Loopback listener (REQ-AUTO.CTRL.WIRE-001, -002, -005, -006): one thread per
//! connection, bounded reads with deadlines, and the discovery file's lifetime.

use crate::discovery::{self, Discovery};
use crate::error::{StartError, WireError};
use crate::frame::{Frame, LineReader};
use crate::msg::{
    hello_event, hello_versions, negotiate, parse_request, reply_limit, request_limit, Reply, ReplyBody, Request,
    HELLO_OP, PROTOCOL_VERSIONS,
};
use std::collections::BTreeMap;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default connection limit: 8 [unverified].
pub const DEFAULT_MAX_CONNECTIONS: usize = 8;
/// How long the accept loop sleeps when no connection is waiting.
const ACCEPT_POLL: Duration = Duration::from_millis(5);
/// Most bytes read while discarding input before a close.
const DRAIN_BUDGET: usize = 32 * 1024 * 1024;
/// Concurrent refusals of over-limit connections.
const MAX_REFUSERS: usize = 8;
/// Bytes and time spent discarding a refused peer's input (before and after the reply).
const REFUSE_DRAIN_BYTES: usize = 64 * 1024;
const REFUSE_DRAIN_TIME: Duration = Duration::from_millis(250);

/// Receives the server's log events, as `(code, detail)`.
pub trait Log: Send + Sync {
    fn log(&self, code: &str, detail: &str);
}

/// A log that keeps events in memory (test harnesses, diagnostics).
#[derive(Debug, Default)]
pub struct MemoryLog(Mutex<Vec<(String, String)>>);

impl MemoryLog {
    /// All events so far.
    pub fn entries(&self) -> Vec<(String, String)> {
        lock(&self.0).clone()
    }

    /// True when an event with `code` was logged.
    pub fn contains(&self, code: &str) -> bool {
        lock(&self.0).iter().any(|(c, _)| c == code)
    }
}

impl Log for MemoryLog {
    fn log(&self, code: &str, detail: &str) {
        lock(&self.0).push((code.to_string(), detail.to_string()));
    }
}

/// A log that writes one line per event to standard error.
#[derive(Debug, Default, Clone, Copy)]
pub struct StderrLog;

impl Log for StderrLog {
    fn log(&self, code: &str, detail: &str) {
        eprintln!("fid-ctrl: {code}: {detail}");
    }
}

/// Answers every request other than `hello` (operations live in other nodes).
pub trait Handler: Send + Sync {
    fn handle(&self, req: &Request) -> ReplyBody;
}

/// Listener configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// `ctrl.address`: must be `127.0.0.1` or `::1`. The port is always ephemeral.
    pub address: String,
    /// Per-user runtime directory for the discovery file (location per OS: PLAT).
    pub discovery_dir: PathBuf,
    /// Most concurrent connections.
    pub max_connections: usize,
    /// Longest wait for the first byte of the first request after connecting. Short, so
    /// unauthenticated sockets cannot hold connection slots; AUTH must complete
    /// authentication within it.
    pub first_request_timeout: Duration,
    /// Longest wait for the first byte of each later line.
    pub idle_timeout: Duration,
    /// Longest time from the first byte of a line to its newline (stall bound).
    pub line_timeout: Duration,
    /// Longest blocking write of a reply.
    pub write_timeout: Duration,
    /// Longest time spent discarding input before closing a refused connection.
    pub drain_timeout: Duration,
}

impl Config {
    /// Defaults: 8 connections, 10 s to the first request, 300 s idle, 10 s per line,
    /// 10 s per write, 2 s drain (the timeouts are this crate's choice [unverified]).
    pub fn new(address: &str, discovery_dir: &Path) -> Self {
        Config {
            address: address.to_string(),
            discovery_dir: discovery_dir.to_path_buf(),
            max_connections: DEFAULT_MAX_CONNECTIONS,
            first_request_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(300),
            line_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(10),
            drain_timeout: Duration::from_secs(2),
        }
    }
}

/// Accepts only the exact loopback addresses `127.0.0.1` and `::1`.
pub fn loopback_address(s: &str) -> Option<IpAddr> {
    match s.parse::<IpAddr>().ok()? {
        a @ IpAddr::V4(v4) if v4 == Ipv4Addr::LOCALHOST => Some(a),
        a @ IpAddr::V6(v6) if v6 == Ipv6Addr::LOCALHOST => Some(a),
        _ => None,
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

struct Shared {
    config: Config,
    instance_id: String,
    handler: Arc<dyn Handler>,
    log: Arc<dyn Log>,
    stop: AtomicBool,
    active: AtomicUsize,
    refusing: AtomicUsize,
    next_conn: AtomicU64,
    conns: Mutex<BTreeMap<u64, TcpStream>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

/// A running control-channel listener. [`Server::stop`] (or drop) closes every
/// connection and deletes the discovery file.
pub struct Server {
    local_addr: SocketAddr,
    discovery_path: PathBuf,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

impl Server {
    /// Validates the address, binds an ephemeral loopback port, writes the discovery
    /// file and starts accepting. A non-loopback address logs `ctrl_non_loopback` and
    /// returns before any socket exists.
    pub fn start(config: Config, handler: Arc<dyn Handler>, log: Arc<dyn Log>) -> Result<Server, StartError> {
        let Some(ip) = loopback_address(&config.address) else {
            log.log(WireError::NonLoopback.code(), &format!("refusing to listen on {:?}", config.address));
            return Err(StartError::NonLoopback(config.address));
        };
        let listener = TcpListener::bind(SocketAddr::new(ip, 0))?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        let instance_id = new_instance_id(local_addr.port());
        let record = Discovery::new(ip, local_addr.port(), &instance_id);
        let discovery_path = discovery::write(&config.discovery_dir, &record)?;
        log.log("ctrl_listening", &format!("{local_addr} instance {instance_id}"));
        let shared = Arc::new(Shared {
            config,
            instance_id,
            handler,
            log,
            stop: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            refusing: AtomicUsize::new(0),
            next_conn: AtomicU64::new(0),
            conns: Mutex::new(BTreeMap::new()),
            workers: Mutex::new(Vec::new()),
        });
        let s = Arc::clone(&shared);
        let accept = thread::Builder::new().name("fid-ctrl-accept".into()).spawn(move || accept_loop(&listener, &s));
        let accept = match accept {
            Ok(h) => h,
            Err(e) => {
                let _ = std::fs::remove_file(&discovery_path);
                return Err(StartError::Io(e));
            }
        };
        Ok(Server { local_addr, discovery_path, shared, accept: Some(accept) })
    }

    /// The bound loopback address and port.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Path of the discovery file.
    pub fn discovery_path(&self) -> &Path {
        &self.discovery_path
    }

    /// This app instance's ID.
    pub fn instance_id(&self) -> &str {
        &self.shared.instance_id
    }

    /// Connections currently being served.
    pub fn active_connections(&self) -> usize {
        self.shared.active.load(Ordering::SeqCst)
    }

    /// Clean shutdown: stop accepting, close every connection, join every thread and
    /// delete the discovery file.
    pub fn stop(mut self) -> io::Result<()> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> io::Result<()> {
        let Some(accept) = self.accept.take() else {
            return Ok(());
        };
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = accept.join();
        for s in lock(&self.shared.conns).values() {
            let _ = s.shutdown(Shutdown::Both);
        }
        let workers: Vec<_> = lock(&self.shared.workers).drain(..).collect();
        for w in workers {
            let _ = w.join();
        }
        self.shared.log.log("ctrl_stopped", &self.local_addr.to_string());
        match std::fs::remove_file(&self.discovery_path) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// A unique, non-secret instance ID: 32 hex digits of SHA-256 over time, process and a counter.
fn new_instance_id(port: u16) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let seed = format!("{nanos}:{}:{n}:{port}", std::process::id());
    let d = fid_canon::digest_bytes(seed.as_bytes());
    d.0[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>) {
    while !shared.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => admit(stream, peer, shared),
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted => {
                thread::sleep(ACCEPT_POLL)
            }
            Err(e) => {
                shared.log.log("ctrl_accept_error", &e.to_string());
                thread::sleep(ACCEPT_POLL);
            }
        }
        lock(&shared.workers).retain(|w| !w.is_finished());
    }
}

/// Decrements the active count and unregisters the connection when its thread ends.
struct Slot {
    shared: Arc<Shared>,
    id: u64,
}

impl Drop for Slot {
    fn drop(&mut self) {
        lock(&self.shared.conns).remove(&self.id);
        self.shared.active.fetch_sub(1, Ordering::SeqCst);
    }
}

fn admit(stream: TcpStream, peer: SocketAddr, shared: &Arc<Shared>) {
    // Accepted sockets may inherit non-blocking mode (Windows, BSD); serve them blocking.
    if stream.set_nonblocking(false).is_err() || stream.set_write_timeout(Some(shared.config.write_timeout)).is_err() {
        return;
    }
    if !peer.ip().is_loopback() {
        shared.log.log(WireError::NonLoopback.code(), &format!("dropping peer {peer}"));
        return;
    }
    if shared.active.load(Ordering::SeqCst) >= shared.config.max_connections {
        shared.log.log(WireError::TooManyConnections.code(), &peer.to_string());
        // Refuse off the accept thread, so a refused peer cannot stall accepting. At
        // most MAX_REFUSERS refusals run at once; beyond that the socket is just dropped.
        if shared.refusing.fetch_add(1, Ordering::SeqCst) >= MAX_REFUSERS {
            shared.refusing.fetch_sub(1, Ordering::SeqCst);
            return;
        }
        let s = Arc::clone(shared);
        let spawned = thread::Builder::new().name("fid-ctrl-refuse".into()).spawn(move || {
            refuse(stream);
            s.refusing.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            shared.refusing.fetch_sub(1, Ordering::SeqCst);
        }
        return;
    }
    let Ok(registered) = stream.try_clone() else {
        return;
    };
    let id = shared.next_conn.fetch_add(1, Ordering::SeqCst);
    shared.active.fetch_add(1, Ordering::SeqCst);
    lock(&shared.conns).insert(id, registered);
    let slot = Slot { shared: Arc::clone(shared), id };
    let worker = thread::Builder::new().name(format!("fid-ctrl-conn-{id}")).spawn(move || {
        let slot = slot;
        serve(stream, &slot.shared);
    });
    // On spawn failure the closure (and its Slot) is dropped, which unregisters it.
    if let Ok(w) = worker {
        lock(&shared.workers).push(w);
    }
}

/// Sends `too_many_connections` and closes. Bytes the peer already sent (a `hello`, say)
/// are read and discarded first, bounded in size and time, so closing does not reset
/// the connection and destroy the reply (Windows resets on close with unread data).
fn refuse(mut stream: TcpStream) {
    discard_briefly(&mut stream);
    if let Ok(line) = Reply::error(None, WireError::TooManyConnections, "connection limit reached").encode() {
        let _ = stream.write_all(&line);
    }
    let _ = stream.shutdown(Shutdown::Write);
    discard_briefly(&mut stream);
}

/// Polls in non-blocking mode (no receive timeouts, which leave a Windows socket in an
/// undefined state), then restores blocking mode for the write.
fn discard_briefly(stream: &mut TcpStream) {
    if stream.set_nonblocking(true).is_err() {
        return;
    }
    let end = Instant::now() + REFUSE_DRAIN_TIME;
    let mut buf = [0u8; 4096];
    let mut seen = 0usize;
    while seen < REFUSE_DRAIN_BYTES && Instant::now() < end {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => seen += n,
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted => {
                thread::sleep(ACCEPT_POLL)
            }
            Err(_) => break,
        }
    }
    let _ = stream.set_nonblocking(false);
}

/// A socket reader with a deadline: `idle` until the first byte of a line, then `line`.
struct Deadline {
    stream: TcpStream,
    idle: Duration,
    line: Duration,
    deadline: Instant,
    in_line: bool,
}

impl Deadline {
    fn arm(&mut self, in_line: bool) {
        self.in_line = in_line;
        self.deadline = Instant::now() + if in_line { self.line } else { self.idle };
    }
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let now = Instant::now();
        let left = self.deadline.saturating_duration_since(now);
        if left.is_zero() {
            return Err(ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
        match self.stream.read(buf) {
            Ok(n) => {
                if n > 0 && !self.in_line {
                    self.arm(true);
                }
                Ok(n)
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => Err(ErrorKind::TimedOut.into()),
            Err(e) => Err(e),
        }
    }
}

enum Next {
    Continue,
    Close,
}

fn serve(stream: TcpStream, shared: &Arc<Shared>) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let cfg = &shared.config;
    let mut writer = stream;
    let mut reader = LineReader::new(Deadline {
        stream: read_half,
        idle: cfg.first_request_timeout,
        line: cfg.line_timeout,
        deadline: Instant::now(),
        in_line: false,
    });
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let pending = reader.has_pending();
        reader.get_mut().arm(pending);
        match reader.next_line(request_limit("")) {
            Ok(Frame::Line(line)) => {
                reader.get_mut().idle = cfg.idle_timeout;
                if let Next::Close = on_line(&line, &mut writer, shared) {
                    close(&mut reader, &mut writer, cfg.drain_timeout);
                    return;
                }
            }
            Ok(Frame::TooLarge) => {
                shared.log.log(WireError::MessageTooLarge.code(), "request line over the bound");
                let _ = send(&mut writer, &Reply::error(None, WireError::MessageTooLarge, "line exceeds 1 MiB"), "");
                reader.get_mut().line = cfg.drain_timeout;
                reader.get_mut().arm(true);
                let _ = reader.discard_line(DRAIN_BUDGET);
                close(&mut reader, &mut writer, cfg.drain_timeout);
                return;
            }
            Ok(Frame::Eof) => return,
            Ok(Frame::PartialEof) => {
                shared.log.log("ctrl_partial_frame", "peer closed mid-line");
                return;
            }
            Err(e) if e.kind() == ErrorKind::TimedOut => {
                shared.log.log(WireError::ReadTimeout.code(), "line or idle deadline passed");
                let _ = send(&mut writer, &Reply::error(None, WireError::ReadTimeout, "read deadline passed"), "");
                close(&mut reader, &mut writer, cfg.drain_timeout);
                return;
            }
            Err(_) => return,
        }
    }
}

/// Half-closes, discards what the peer still sends (bounded), then drops the socket, so
/// the peer reads the last reply instead of a connection reset.
fn close(reader: &mut LineReader<Deadline>, writer: &mut TcpStream, drain_timeout: Duration) {
    let _ = writer.shutdown(Shutdown::Write);
    let d = reader.get_mut();
    d.line = drain_timeout;
    d.idle = drain_timeout;
    d.arm(false);
    let _ = reader.drain(DRAIN_BUDGET);
}

fn send(writer: &mut TcpStream, reply: &Reply, op: &str) -> io::Result<()> {
    let line = match reply.encode() {
        Ok(l) if l.len().saturating_sub(1) <= reply_limit(op) => l,
        Ok(_) => Reply::error(reply.id, WireError::MessageTooLarge, "reply exceeds its bound")
            .encode()
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e.code()))?,
        Err(e) => Reply::error(reply.id, e, "reply could not be encoded")
            .encode()
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e.code()))?,
    };
    writer.write_all(&line)?;
    writer.flush()
}

fn on_line(line: &[u8], writer: &mut TcpStream, shared: &Arc<Shared>) -> Next {
    let ok = |r: io::Result<()>| if r.is_ok() { Next::Continue } else { Next::Close };
    let req = match parse_request(line) {
        Ok(r) => r,
        Err(m) => {
            shared.log.log(WireError::MalformedMessage.code(), &m.reason);
            return ok(send(writer, &Reply::error(m.id, WireError::MalformedMessage, &m.reason), ""));
        }
    };
    if !PROTOCOL_VERSIONS.contains(&req.v) {
        shared.log.log(WireError::VersionUnsupported.code(), &format!("request v {}", req.v));
        let _ = send(writer, &Reply::error(Some(req.id), WireError::VersionUnsupported, "unsupported v"), "");
        return Next::Close;
    }
    if req.op.as_str() == HELLO_OP {
        let versions = match hello_versions(&req) {
            Ok(v) => v,
            Err(m) => {
                shared.log.log(WireError::MalformedMessage.code(), &m.reason);
                return ok(send(writer, &Reply::error(m.id, WireError::MalformedMessage, &m.reason), ""));
            }
        };
        let Some(v) = negotiate(&versions) else {
            shared.log.log(WireError::VersionUnsupported.code(), &format!("client versions {versions:?}"));
            let msg = "no common protocol version";
            let _ = send(writer, &Reply::error(Some(req.id), WireError::VersionUnsupported, msg), "");
            return Next::Close;
        };
        let reply = Reply { v, id: Some(req.id), body: ReplyBody::Event(hello_event(v, &shared.instance_id)) };
        return ok(send(writer, &reply, HELLO_OP));
    }
    let body = shared.handler.handle(&req);
    ok(send(writer, &Reply { v: req.v, id: Some(req.id), body }, req.op.as_str()))
}
