//! Control-channel client (spec node `AUTO.CTRL.CLIENT`): discovery, the client side of the
//! handshake, request multiplexing and honest disconnect handling.
//!
//! [`CtrlClient::connect`] reads one discovery file, refuses any address other than `127.0.0.1`
//! or `::1` (`ctrl_non_loopback`) before opening a socket, sends `hello`, and checks that
//! `hello.instance_id` is the instance ID of that discovery file (`instance_mismatch`) before it
//! sends `auth`. The token is read from the token file for every connection (the app rotates it
//! at every start), through [`TokenFile::read_checked`]: opened without following links, a
//! regular file, owner-only, checked on the handle, never replaced by the client. The client
//! proof is [`client_proof`]; the server's answer is checked with [`verify_server_proof`], and a
//! wrong, missing or malformed server proof closes the connection with `server_auth_failed`
//! before any operation is sent.
//!
//! After authentication a reader thread frames reply lines (bounded at 16 MiB, the screenshot
//! bound; every other reply is held to 1 MiB when it is matched to its request) into a bounded
//! queue. The client implements [`BusTransport`], so the `AUTO.SURF.JOBS` [`Tracker`] runs over
//! it and does the `not_run` / `no_reply` classification. Many requests may be in flight at once;
//! replies are matched by request ID.
//!
//! # Wire contract for operations (the client's side; the server side is `AUTO.CTRL.EXEC`)
//!
//! - [`Purpose::Submit`] and [`Purpose::Wait`] are sent as op `command.execute`,
//!   [`Purpose::Cancel`] as op `job.cancel`, with `params = {command, params, key, attempt}`:
//!   the command ID, its parameters, the idempotency key (UUID text) and the attempt number.
//! - Replies with the request's ID: an event `{kind: "accepted", job}` when the bus accepts it;
//!   events `{kind: "progress", fraction, stage}`; then exactly one envelope, the final state.
//!   An `{kind: "error", code, ..}` event before acceptance means the server did not and will not
//!   run the request ([`BusEvent::Refused`]); after acceptance it is ignored.
//! - A final envelope for a `Wait` request is reported under the idempotency key of the request
//!   that was accepted as that job on this connection, when there is one.
//!
//! # Disconnect
//!
//! When the connection drops, [`BusTransport::recv`] returns [`BusEvent::Disconnected`] once and
//! every later send fails with [`SendError::NotSent`]. The tracker then reports `no_reply` with
//! the job ID for every accepted invocation and, for one not yet accepted, `not_run` only when it
//! is certain nothing of it reached the server (no byte of its line was written, or the server
//! refused it) and `no_reply` otherwise (REQ-AUTO.SURF-007; see the leaf's Open questions).
//!
//! [`Tracker`]: fid_surface::jobs::Tracker

use crate::auth::{client_proof, verify_server_proof, AuthError, Nonce, Proof, AUTH_OP};
use crate::discovery::{self, Discovery};
use crate::frame::{Frame, LineReader};
use crate::msg::{
    reply_limit, Reply, ReplyBody, HELLO_OP, MAX_ID, MAX_MESSAGE_BYTES, MAX_SCREENSHOT_REPLY_BYTES, PROTOCOL_VERSIONS,
    SCREENSHOT_OP,
};
use crate::token::TokenFile;
use fid_canon::{to_canonical_bytes, CValue};
use fid_surface::jobs::{BusEvent, BusRequest, BusTransport, Purpose, SendError};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The execute operation (owned by `AUTO.CTRL.EXEC`).
pub const EXECUTE_OP: &str = "command.execute";
/// The job-cancel operation (owned by `AUTO.CTRL.EXEC`).
pub const CANCEL_OP: &str = "job.cancel";
/// Request ID of the handshake's `hello`.
const HELLO_ID: u64 = 1;
/// Request ID of the handshake's `auth`.
const AUTH_ID: u64 = 2;
/// Requests in flight at once; a send beyond this fails with [`SendError::NotSent`].
pub const MAX_IN_FLIGHT: usize = 4096;
/// Reply lines the reader thread queues before it waits for the client to read them.
pub const INBOX_LINES: usize = 64;
/// Lines one [`BusTransport::recv`] call reads at most while skipping lines that carry no event.
const MAX_LINES_PER_RECV: usize = 4096;
/// Default deadline for connecting and authenticating.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// Default write timeout once connected.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest error code taken over from the server as a refusal reason.
const MAX_CODE_BYTES: usize = 64;

// ------------------------------------------------------------------ errors

/// Why connecting, authenticating or a diagnostic failed. Codes are stable. No variant holds
/// token bytes: the token never leaves [`crate::auth::Token`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// No discovery file, an unreadable one, or nothing listening at its address.
    AppNotFound(String),
    /// The discovery file names an address other than `127.0.0.1` or `::1`; nothing was opened.
    NonLoopback(String),
    /// The server's proof is wrong, missing or malformed; the connection was closed.
    ServerAuthFailed,
    /// The server supports none of the client's versions; the versions it listed.
    VersionUnsupported(Vec<u16>),
    /// `hello.instance_id` is not the instance ID of the discovery file; closed before `auth`.
    InstanceMismatch,
    /// The token file could not be used (`token_permissions` or `token_io`).
    Token(AuthError),
    /// The server answered with an error event (its code, if it is a known one).
    ServerRefused(&'static str),
    /// The server's reply did not follow the protocol.
    Protocol(&'static str),
    /// The deadline passed.
    Timeout,
    /// The connection failed or closed.
    Transport(String),
}

impl ClientError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ClientError::AppNotFound(_) => "app_not_found",
            ClientError::NonLoopback(_) => "ctrl_non_loopback",
            ClientError::ServerAuthFailed => "server_auth_failed",
            ClientError::VersionUnsupported(_) => "version_unsupported",
            ClientError::InstanceMismatch => "instance_mismatch",
            ClientError::Token(e) => e.code(),
            ClientError::ServerRefused(c) => c,
            ClientError::Protocol(_) => "protocol_violation",
            ClientError::Timeout => "ctrl_timeout",
            ClientError::Transport(_) => "transport_error",
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::AppNotFound(why) | ClientError::Transport(why) => write!(f, "{}: {why}", self.code()),
            ClientError::NonLoopback(a) => write!(f, "{}: {a}", self.code()),
            ClientError::VersionUnsupported(v) => write!(f, "{}: the app supports {v:?}", self.code()),
            ClientError::Protocol(why) => write!(f, "{}: {why}", self.code()),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for ClientError {}

/// Known server error codes; anything else is reported as `server_error`.
fn known_code(code: &str) -> &'static str {
    const KNOWN: &[&str] = &[
        "auth_failed",
        "token_permissions",
        "token_io",
        "too_many_connections",
        "version_unsupported",
        "malformed_message",
        "message_too_large",
        "read_timeout",
    ];
    KNOWN.iter().find(|k| **k == code).copied().unwrap_or("server_error")
}

/// A server error code as a refusal reason: kept when it looks like a code (1 to 64 bytes of
/// `[a-z0-9_]`), else `server_error`, so no peer text reaches a report unfiltered.
fn safe_code(code: &str) -> String {
    let ok = !code.is_empty()
        && code.len() <= MAX_CODE_BYTES
        && code.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        code.to_string()
    } else {
        "server_error".to_string()
    }
}

fn transport(e: &io::Error) -> ClientError {
    match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => ClientError::Timeout,
        k => ClientError::Transport(format!("{k:?}")),
    }
}

/// Accepts only the exact loopback addresses `127.0.0.1` and `::1` (REQ-AUTO.CTRL.CLIENT-001).
pub fn is_allowed_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4 == Ipv4Addr::LOCALHOST,
        IpAddr::V6(v6) => v6 == Ipv6Addr::LOCALHOST,
    }
}

// ------------------------------------------------------------------ socket reads with a deadline

/// A socket reader that fails with `TimedOut` once `deadline` has passed; no deadline blocks.
struct Timed {
    stream: TcpStream,
    deadline: Option<Instant>,
}

impl Read for Timed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.deadline {
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(ErrorKind::TimedOut.into());
                }
                self.stream.set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
            }
            None => self.stream.set_read_timeout(None)?,
        }
        match self.stream.read(buf) {
            Err(e) if e.kind() == ErrorKind::WouldBlock => Err(ErrorKind::TimedOut.into()),
            other => other,
        }
    }
}

// ------------------------------------------------------------------ request encoding

fn num(n: u64) -> CValue {
    CValue::Num(n as f64)
}

/// One request line `{v, id, op, params}`, newline included.
fn request_line(v: u16, id: u64, op: &str, params: CValue) -> Option<Vec<u8>> {
    let mut o = BTreeMap::new();
    o.insert("v".to_string(), num(u64::from(v)));
    o.insert("id".to_string(), num(id));
    o.insert("op".to_string(), CValue::Str(op.to_string()));
    o.insert("params".to_string(), params);
    let mut line = to_canonical_bytes(&CValue::Obj(o)).ok()?;
    if line.len() > MAX_MESSAGE_BYTES {
        return None;
    }
    line.push(b'\n');
    Some(line)
}

fn hello_params() -> CValue {
    let versions = PROTOCOL_VERSIONS.iter().map(|&v| num(u64::from(v))).collect();
    CValue::Obj(BTreeMap::from([("versions".to_string(), CValue::Arr(versions))]))
}

/// The operation and parameters a [`BusRequest`] is sent as (see the module documentation).
fn op_request(r: &BusRequest) -> (&'static str, CValue) {
    let op = match r.purpose {
        Purpose::Submit | Purpose::Wait => EXECUTE_OP,
        Purpose::Cancel => CANCEL_OP,
    };
    let mut o = BTreeMap::new();
    o.insert("command".to_string(), CValue::Str(r.command.as_str().to_string()));
    o.insert("params".to_string(), r.params.clone());
    o.insert("key".to_string(), CValue::Str(r.key.to_string()));
    o.insert("attempt".to_string(), num(u64::from(r.attempt)));
    (op, CValue::Obj(o))
}

/// The `job` parameter of a wait or cancel request.
fn job_param(r: &BusRequest) -> Option<Uuid> {
    match &r.params {
        CValue::Obj(o) => match o.get("job") {
            Some(CValue::Str(s)) => Uuid::parse(s).ok(),
            _ => None,
        },
        _ => None,
    }
}

// ------------------------------------------------------------------ reply decoding helpers

fn event_obj(r: &Reply) -> Option<&BTreeMap<String, CValue>> {
    match &r.body {
        ReplyBody::Event(CValue::Obj(o)) => Some(o),
        _ => None,
    }
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Option<&'a str> {
    match o.get(k) {
        Some(CValue::Str(s)) => Some(s),
        _ => None,
    }
}

fn uint_field(o: &BTreeMap<String, CValue>, k: &str, max: u64) -> Option<u64> {
    match o.get(k) {
        Some(CValue::Num(f)) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 && *f <= max as f64 => Some(*f as u64),
        _ => None,
    }
}

fn versions_field(o: &BTreeMap<String, CValue>) -> Vec<u16> {
    match o.get("versions") {
        Some(CValue::Arr(a)) => a
            .iter()
            .take(64)
            .filter_map(|v| match v {
                CValue::Num(f) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 && *f <= f64::from(u16::MAX) => {
                    Some(*f as u16)
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The error a handshake reply's error event stands for.
fn handshake_error(o: &BTreeMap<String, CValue>) -> ClientError {
    let code = str_field(o, "code").unwrap_or("");
    if code == "version_unsupported" {
        return ClientError::VersionUnsupported(versions_field(o));
    }
    ClientError::ServerRefused(known_code(code))
}

// ------------------------------------------------------------------ the client

/// What the reader thread hands the client.
enum Inbound {
    Line(Vec<u8>),
    Closed,
}

/// A request in flight.
#[derive(Debug, Clone)]
struct Pending {
    purpose: Purpose,
    key: Uuid,
    attempt: u32,
    /// The job: from the acceptance (submit), or the request's `job` parameter (wait, cancel).
    job: Option<Uuid>,
    /// True once an acceptance was received for this request.
    accepted: bool,
}

/// Messages the client read and dropped (all saturating).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClientCounters {
    /// Reply lines that do not decode, or exceed their operation's bound.
    pub malformed: u64,
    /// Replies for no request in flight (or with a null ID).
    pub unmatched: u64,
    /// Events that carry nothing the tracker uses (unknown kinds, errors after acceptance, bad
    /// fields).
    pub ignored: u64,
}

fn bump(c: &mut u64) {
    *c = c.saturating_add(1);
}

/// An authenticated connection to a running app.
pub struct CtrlClient {
    writer: TcpStream,
    inbox: Option<Receiver<Inbound>>,
    reader: Option<JoinHandle<()>>,
    discovery: Discovery,
    version: u16,
    next_id: u64,
    pending: BTreeMap<u64, Pending>,
    /// Submit key of each job accepted on this connection (for wait finals).
    jobs: BTreeMap<Uuid, Uuid>,
    ready: VecDeque<BusEvent>,
    closed: bool,
    disconnect_reported: bool,
    counters: ClientCounters,
}

impl fmt::Debug for CtrlClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CtrlClient")
            .field("instance_id", &self.discovery.instance_id)
            .field("version", &self.version)
            .field("in_flight", &self.pending.len())
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl CtrlClient {
    /// Reads the discovery file at `discovery_path`, connects and authenticates with the token
    /// in `token_path`, all within `timeout`.
    pub fn connect(discovery_path: &Path, token_path: &Path, timeout: Duration) -> Result<CtrlClient, ClientError> {
        let deadline = Instant::now() + timeout;
        let record = discovery::read(discovery_path).map_err(|e| {
            ClientError::AppNotFound(format!("discovery file {}: {:?}", discovery_path.display(), e.kind()))
        })?;
        CtrlClient::connect_record(record, token_path, deadline)
    }

    /// Connects to the app a decoded discovery record names (see [`CtrlClient::connect`]).
    pub fn connect_record(record: Discovery, token_path: &Path, deadline: Instant) -> Result<CtrlClient, ClientError> {
        // REQ-AUTO.CTRL.CLIENT-001: refuse before any socket exists.
        if !is_allowed_address(record.address) {
            return Err(ClientError::NonLoopback(record.address.to_string()));
        }
        let addr = SocketAddr::new(record.address, record.port);
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ClientError::Timeout);
        }
        let stream = match TcpStream::connect_timeout(&addr, left) {
            Ok(s) => s,
            Err(e) if matches!(e.kind(), ErrorKind::ConnectionRefused | ErrorKind::AddrNotAvailable) => {
                return Err(ClientError::AppNotFound(format!("nothing listening at {addr}")))
            }
            Err(e) => return Err(transport(&e)),
        };
        match stream.peer_addr() {
            Ok(p) if is_allowed_address(p.ip()) => {}
            Ok(p) => return Err(ClientError::NonLoopback(p.ip().to_string())),
            Err(e) => return Err(transport(&e)),
        }
        let _ = stream.set_nodelay(true);
        stream.set_write_timeout(Some(WRITE_TIMEOUT)).map_err(|e| transport(&e))?;
        let read_half = stream.try_clone().map_err(|e| transport(&e))?;
        let mut reader = LineReader::new(Timed { stream: read_half, deadline: Some(deadline) });
        let mut writer = stream;
        match handshake(&mut reader, &mut writer, &record, token_path) {
            Ok(version) => {
                reader.get_mut().deadline = None;
                let (tx, rx) = mpsc::sync_channel(INBOX_LINES);
                let spawned =
                    thread::Builder::new().name("fid-ctrl-client".into()).spawn(move || read_loop(reader, &tx));
                let handle = match spawned {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = writer.shutdown(Shutdown::Both);
                        return Err(transport(&e));
                    }
                };
                Ok(CtrlClient {
                    writer,
                    inbox: Some(rx),
                    reader: Some(handle),
                    discovery: record,
                    version,
                    next_id: AUTH_ID + 1,
                    pending: BTreeMap::new(),
                    jobs: BTreeMap::new(),
                    ready: VecDeque::new(),
                    closed: false,
                    disconnect_reported: false,
                    counters: ClientCounters::default(),
                })
            }
            Err(e) => {
                // REQ-AUTO.CTRL.CLIENT-002: nothing more is sent; the socket is closed now.
                let _ = writer.shutdown(Shutdown::Both);
                Err(e)
            }
        }
    }

    /// The negotiated protocol version.
    pub fn version(&self) -> u16 {
        self.version
    }

    /// The discovery record this client connected from.
    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    /// Requests sent and not yet finished (or refused).
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// True once the connection is known to be closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Messages read and dropped so far.
    pub fn counters(&self) -> ClientCounters {
        self.counters
    }

    /// An authenticated ping (REQ-AUTO.CTRL.CLIENT-005): a `hello` on the authenticated
    /// connection, which the server answers without running any registry command. Returns the
    /// round-trip time. Replies to other requests read meanwhile are kept for
    /// [`BusTransport::recv`].
    pub fn ping(&mut self, timeout: Duration) -> Result<Duration, ClientError> {
        if self.closed {
            return Err(ClientError::Transport("connection closed".into()));
        }
        let id = self.take_id().ok_or(ClientError::Protocol("request IDs exhausted"))?;
        let line = request_line(self.version, id, HELLO_OP, hello_params()).ok_or(ClientError::Protocol("encode"))?;
        let start = Instant::now();
        let deadline = start + timeout;
        if let Err(e) = self.writer.write_all(&line).and_then(|()| self.writer.flush()) {
            self.mark_closed();
            return Err(transport(&e));
        }
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ClientError::Timeout);
            }
            let Some(inbox) = &self.inbox else {
                return Err(ClientError::Transport("connection closed".into()));
            };
            match inbox.recv_timeout(left) {
                Ok(Inbound::Line(l)) => {
                    let Ok(reply) = Reply::decode(&l, SCREENSHOT_OP) else {
                        bump(&mut self.counters.malformed);
                        continue;
                    };
                    if reply.id != Some(id) {
                        if let Some(ev) = self.on_reply(reply, l.len()) {
                            self.ready.push_back(ev);
                        }
                        continue;
                    }
                    let rtt = start.elapsed();
                    let Some(o) = event_obj(&reply) else {
                        return Err(ClientError::Protocol("ping reply is not an event"));
                    };
                    return match str_field(o, "kind") {
                        Some("hello") if str_field(o, "instance_id") == Some(self.discovery.instance_id.as_str()) => {
                            Ok(rtt)
                        }
                        Some("hello") => Err(ClientError::InstanceMismatch),
                        Some("error") => Err(handshake_error(o)),
                        _ => Err(ClientError::Protocol("unexpected ping reply")),
                    };
                }
                Ok(Inbound::Closed) | Err(RecvTimeoutError::Disconnected) => {
                    if let Some(ev) = self.mark_closed() {
                        self.ready.push_back(ev);
                    }
                    return Err(ClientError::Transport("connection closed".into()));
                }
                Err(RecvTimeoutError::Timeout) => return Err(ClientError::Timeout),
            }
        }
    }

    /// Closes the connection. In-flight requests are reported by the tracker as on a drop.
    pub fn close(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.writer.shutdown(Shutdown::Both);
        self.closed = true;
        // Dropping the receiver first unblocks a reader waiting on a full queue.
        self.inbox = None;
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
    }

    fn take_id(&mut self) -> Option<u64> {
        let id = self.next_id;
        if id > MAX_ID {
            return None;
        }
        self.next_id += 1;
        Some(id)
    }

    /// Marks the connection closed; returns [`BusEvent::Disconnected`] the first time.
    fn mark_closed(&mut self) -> Option<BusEvent> {
        let _ = self.writer.shutdown(Shutdown::Both);
        self.closed = true;
        self.pending.clear();
        if self.disconnect_reported {
            return None;
        }
        self.disconnect_reported = true;
        Some(BusEvent::Disconnected)
    }

    /// Turns one decoded reply into the event it stands for, if any.
    fn on_reply(&mut self, reply: Reply, len: usize) -> Option<BusEvent> {
        let Some(id) = reply.id else {
            bump(&mut self.counters.unmatched);
            return None;
        };
        let Some(p) = self.pending.get(&id).cloned() else {
            bump(&mut self.counters.unmatched);
            return None;
        };
        // Only a screenshot reply may use the 16 MiB bound; no operation here is one.
        if len > reply_limit(EXECUTE_OP) {
            bump(&mut self.counters.malformed);
            return None;
        }
        match reply.body {
            ReplyBody::Envelope(envelope) => {
                self.pending.remove(&id);
                let job = p.job.or_else(|| envelope.job_id.as_deref().and_then(|j| Uuid::parse(j).ok()));
                // Before any acceptance there is no job; the tracker treats the final state as
                // a protocol violation (finish before accept), whatever job is named.
                let job = job.unwrap_or(Uuid::from_u128(0));
                let key = match p.purpose {
                    Purpose::Wait => self.jobs.get(&job).copied().unwrap_or(p.key),
                    Purpose::Submit | Purpose::Cancel => p.key,
                };
                Some(BusEvent::Finished { key, job, envelope })
            }
            ReplyBody::Event(CValue::Obj(o)) => match str_field(&o, "kind") {
                Some("accepted") => {
                    let Some(job) = str_field(&o, "job").and_then(|j| Uuid::parse(j).ok()) else {
                        bump(&mut self.counters.ignored);
                        return None;
                    };
                    if let Some(entry) = self.pending.get_mut(&id) {
                        entry.accepted = true;
                        if entry.job.is_none() {
                            entry.job = Some(job);
                        }
                    }
                    match p.purpose {
                        Purpose::Submit => {
                            self.jobs.entry(job).or_insert(p.key);
                            Some(BusEvent::Accepted { key: p.key, job })
                        }
                        // A wait or cancel is accepted for a job already known; nothing to add.
                        Purpose::Wait | Purpose::Cancel => None,
                    }
                }
                Some("progress") => {
                    let fraction = match o.get("fraction") {
                        Some(CValue::Num(f)) => *f,
                        _ => {
                            bump(&mut self.counters.ignored);
                            return None;
                        }
                    };
                    let stage = str_field(&o, "stage").unwrap_or("").to_string();
                    let job = p.job.or_else(|| str_field(&o, "job").and_then(|j| Uuid::parse(j).ok()));
                    match job {
                        Some(job) => Some(BusEvent::Progress { job, fraction, stage }),
                        None => {
                            bump(&mut self.counters.ignored);
                            None
                        }
                    }
                }
                Some("error") if !p.accepted => {
                    self.pending.remove(&id);
                    let code = str_field(&o, "code").unwrap_or("");
                    Some(BusEvent::Refused { key: p.key, attempt: p.attempt, reason: safe_code(code) })
                }
                _ => {
                    bump(&mut self.counters.ignored);
                    None
                }
            },
            ReplyBody::Event(_) => {
                bump(&mut self.counters.ignored);
                None
            }
        }
    }
}

impl Drop for CtrlClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl BusTransport for CtrlClient {
    fn send(&mut self, request: &BusRequest) -> Result<(), SendError> {
        if self.closed || self.pending.len() >= MAX_IN_FLIGHT {
            return Err(SendError::NotSent);
        }
        let (op, params) = op_request(request);
        let Some(id) = self.take_id() else {
            return Err(SendError::NotSent);
        };
        let Some(line) = request_line(self.version, id, op, params) else {
            return Err(SendError::NotSent);
        };
        let job = match request.purpose {
            Purpose::Submit => None,
            Purpose::Wait | Purpose::Cancel => job_param(request),
        };
        let mut written = 0usize;
        while written < line.len() {
            match self.writer.write(&line[written..]) {
                Ok(0) => break,
                Ok(n) => written += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        if written == line.len() && self.writer.flush().is_ok() {
            self.pending.insert(
                id,
                Pending { purpose: request.purpose, key: request.key, attempt: request.attempt, job, accepted: false },
            );
            return Ok(());
        }
        // The connection is broken. A line with no byte written certainly did not leave.
        if let Some(ev) = self.mark_closed() {
            self.ready.push_back(ev);
        }
        if written == 0 {
            Err(SendError::NotSent)
        } else {
            Err(SendError::Unknown)
        }
    }

    fn recv(&mut self) -> Option<BusEvent> {
        if let Some(ev) = self.ready.pop_front() {
            return Some(ev);
        }
        for _ in 0..MAX_LINES_PER_RECV {
            let next = match &self.inbox {
                Some(inbox) => inbox.try_recv(),
                None => Err(TryRecvError::Disconnected),
            };
            match next {
                Ok(Inbound::Line(l)) => match Reply::decode(&l, SCREENSHOT_OP) {
                    Ok(reply) => {
                        if let Some(ev) = self.on_reply(reply, l.len()) {
                            return Some(ev);
                        }
                    }
                    Err(_) => bump(&mut self.counters.malformed),
                },
                Ok(Inbound::Closed) | Err(TryRecvError::Disconnected) => return self.mark_closed(),
                Err(TryRecvError::Empty) => return None,
            }
        }
        None
    }
}

/// The reader thread: frames reply lines until the connection ends or the client goes away.
fn read_loop(mut reader: LineReader<Timed>, tx: &SyncSender<Inbound>) {
    loop {
        match reader.next_line(MAX_SCREENSHOT_REPLY_BYTES) {
            Ok(Frame::Line(l)) => {
                if tx.send(Inbound::Line(l)).is_err() {
                    return;
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            _ => {
                let _ = tx.send(Inbound::Closed);
                return;
            }
        }
    }
}

// ------------------------------------------------------------------ handshake

fn send_line(writer: &mut TcpStream, line: &[u8]) -> Result<(), ClientError> {
    writer.write_all(line).and_then(|()| writer.flush()).map_err(|e| transport(&e))
}

/// Reads the reply to the handshake request `id` (of operation `op`).
fn read_reply(reader: &mut LineReader<Timed>, id: u64, op: &str) -> Result<Reply, ClientError> {
    match reader.next_line(MAX_MESSAGE_BYTES) {
        Ok(Frame::Line(l)) => {
            let reply = Reply::decode(&l, op).map_err(|_| ClientError::Protocol("reply does not decode"))?;
            // A server error before it could read the request ID carries a null ID.
            if reply.id.is_some() && reply.id != Some(id) {
                return Err(ClientError::Protocol("reply for another request"));
            }
            Ok(reply)
        }
        Ok(Frame::TooLarge) => Err(ClientError::Protocol("reply line too large")),
        Ok(Frame::Eof | Frame::PartialEof) => Err(ClientError::Transport("connection closed during handshake".into())),
        Err(e) => Err(transport(&e)),
    }
}

/// `hello`, the instance check, `auth` and the server-proof check. Returns the version.
fn handshake(
    reader: &mut LineReader<Timed>,
    writer: &mut TcpStream,
    record: &Discovery,
    token_path: &Path,
) -> Result<u16, ClientError> {
    let v = PROTOCOL_VERSIONS[0];
    let hello = request_line(v, HELLO_ID, HELLO_OP, hello_params()).ok_or(ClientError::Protocol("encode"))?;
    send_line(writer, &hello)?;
    let reply = read_reply(reader, HELLO_ID, HELLO_OP)?;
    let o = event_obj(&reply).ok_or(ClientError::Protocol("hello reply is not an event"))?;
    match str_field(o, "kind") {
        Some("hello") => {}
        Some("error") => return Err(handshake_error(o)),
        _ => return Err(ClientError::Protocol("unexpected hello reply")),
    }
    let version = uint_field(o, "version", u64::from(u16::MAX)).map(|n| n as u16);
    let Some(version) = version.filter(|v| PROTOCOL_VERSIONS.contains(v)) else {
        return Err(ClientError::Protocol("hello names a version this client does not speak"));
    };
    // AUTO.CTRL.AUTH, instance binding: the app answering must be the one discovered.
    if str_field(o, "instance_id") != Some(record.instance_id.as_str()) {
        return Err(ClientError::InstanceMismatch);
    }
    let server_nonce =
        str_field(o, "nonce").and_then(Nonce::from_hex).ok_or(ClientError::Protocol("hello without a valid nonce"))?;
    // The token is rotated at every app start: read it for this connection, with the checks.
    let token = TokenFile::open_existing(token_path)
        .map_err(|_| ClientError::Token(AuthError::TokenIo))?
        .read_checked()
        .map_err(ClientError::Token)?;
    let client_nonce = Nonce::random().map_err(|_| ClientError::Transport("OS random source failed".into()))?;
    let proof = client_proof(&token, &record.instance_id, &server_nonce, &client_nonce);
    let params = CValue::Obj(BTreeMap::from([
        ("nonce".to_string(), CValue::Str(client_nonce.to_hex())),
        ("proof".to_string(), CValue::Str(proof.to_hex())),
    ]));
    let auth = request_line(version, AUTH_ID, AUTH_OP, params).ok_or(ClientError::Protocol("encode"))?;
    send_line(writer, &auth)?;
    let reply = read_reply(reader, AUTH_ID, AUTH_OP)?;
    let o = event_obj(&reply).ok_or(ClientError::ServerAuthFailed)?;
    match str_field(o, "kind") {
        Some("auth") => {}
        Some("error") => return Err(handshake_error(o)),
        _ => return Err(ClientError::ServerAuthFailed),
    }
    let server = str_field(o, "proof").and_then(Proof::from_hex).ok_or(ClientError::ServerAuthFailed)?;
    if !verify_server_proof(&token, &record.instance_id, &client_nonce, &server_nonce, &server) {
        return Err(ClientError::ServerAuthFailed);
    }
    Ok(version)
}

// ------------------------------------------------------------------ discovery and diagnostics

/// Where a client looks for the app.
#[derive(Debug, Clone)]
pub struct Target {
    /// The per-user runtime directory holding discovery files (location per OS: PLAT).
    pub runtime_dir: PathBuf,
    /// The app's token file (location per OS: PLAT).
    pub token_path: PathBuf,
    /// Connect only to this instance; otherwise every discovery file is tried in name order.
    pub instance_id: Option<String>,
    /// Deadline for connecting and authenticating to one candidate.
    pub timeout: Duration,
}

/// Connects to the app: the discovery file of `target.instance_id`, or else the first discovery
/// file in name order whose app is listening (stale files of apps that are gone are skipped, as
/// are files that do not decode or name a non-loopback address). `app_not_found` when no
/// candidate is listening; `ctrl_non_loopback` when the only candidates were refused for that.
pub fn connect(target: &Target) -> Result<CtrlClient, ClientError> {
    let paths = match &target.instance_id {
        Some(id) if !discovery::valid_instance_id(id) => {
            return Err(ClientError::AppNotFound("invalid instance ID".into()));
        }
        Some(id) => vec![target.runtime_dir.join(Discovery::file_name(id))],
        None => discovery::candidates(&target.runtime_dir)
            .map_err(|e| ClientError::AppNotFound(format!("runtime directory: {:?}", e.kind())))?,
    };
    let mut last = ClientError::AppNotFound(format!("no discovery file in {}", target.runtime_dir.display()));
    for path in paths {
        let deadline = Instant::now() + target.timeout;
        let record = match discovery::read(&path) {
            Ok(r) => r,
            Err(e) => {
                if !matches!(last, ClientError::NonLoopback(_)) {
                    last = ClientError::AppNotFound(format!("discovery file {}: {:?}", path.display(), e.kind()));
                }
                continue;
            }
        };
        match CtrlClient::connect_record(record, &target.token_path, deadline) {
            Ok(c) => return Ok(c),
            Err(e @ ClientError::NonLoopback(_)) => last = e,
            Err(e @ ClientError::AppNotFound(_)) => {
                if !matches!(last, ClientError::NonLoopback(_)) {
                    last = e;
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// The result of `fid ctrl status` (REQ-AUTO.CTRL.CLIENT-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// An app was reachable at a discovery file's loopback address.
    pub found: bool,
    /// The handshake completed and both proofs verified.
    pub authenticated: bool,
    /// The negotiated protocol version.
    pub version: Option<u16>,
    pub instance_id: Option<String>,
    pub address: Option<SocketAddr>,
    /// Why `found` or `authenticated` is false.
    pub error: Option<ClientError>,
}

/// Connects, authenticates and closes again; changes no state in the app.
pub fn status(target: &Target) -> Status {
    match connect(target) {
        Ok(c) => {
            let d = c.discovery().clone();
            let version = c.version();
            c.close();
            Status {
                found: true,
                authenticated: true,
                version: Some(version),
                instance_id: Some(d.instance_id),
                address: Some(SocketAddr::new(d.address, d.port)),
                error: None,
            }
        }
        Err(e) => Status {
            found: !matches!(e, ClientError::AppNotFound(_) | ClientError::NonLoopback(_)),
            authenticated: false,
            version: None,
            instance_id: None,
            address: None,
            error: Some(e),
        },
    }
}
