//! Message schema (REQ-AUTO.CTRL.WIRE-003 to -005): `{v, id, op, params}` requests and
//! `{v, id, envelope | event}` replies, one canonical JSON object per line.
//!
//! Every function here is pure; [`classify_stream`] is the fuzz entry point.

use crate::error::WireError;
use crate::frame::{Frame, LineReader};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_surface::Envelope;
use std::collections::BTreeMap;
use std::fmt;

/// Protocol versions this build speaks, ascending.
pub const PROTOCOL_VERSIONS: &[u16] = &[1];
/// Largest request or ordinary reply line, newline excluded: 1 MiB [unverified].
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Largest screenshot reply line, newline excluded: 16 MiB [unverified].
pub const MAX_SCREENSHOT_REPLY_BYTES: usize = 16 * 1024 * 1024;
/// Deepest JSON nesting accepted in a request.
pub const MAX_DEPTH: usize = 64;
/// Largest request ID: IDs are JSON numbers and must be exact in an IEEE 754 double.
pub const MAX_ID: u64 = (1 << 53) - 1;
/// Longest operation name, in bytes.
pub const MAX_OP_BYTES: usize = 64;
/// Most protocol versions a `hello` may list.
pub const MAX_HELLO_VERSIONS: usize = 64;
/// The version-negotiation operation.
pub const HELLO_OP: &str = "hello";
/// The screenshot operation, whose reply gets the larger bound. The name is owned by
/// AUTO.CTRL.UIOPS, which is not yet specified [unverified].
pub const SCREENSHOT_OP: &str = "ui.screenshot";

/// Line bound for a request of operation `op` (all requests: 1 MiB).
pub fn request_limit(_op: &str) -> usize {
    MAX_MESSAGE_BYTES
}

/// Line bound for the reply to operation `op`.
pub fn reply_limit(op: &str) -> usize {
    if op == SCREENSHOT_OP {
        MAX_SCREENSHOT_REPLY_BYTES
    } else {
        MAX_MESSAGE_BYTES
    }
}

/// A validated operation name: dot-separated segments of `[a-z][a-z0-9_]*`, at most 64 bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpName(String);

impl OpName {
    /// Validates an operation name.
    pub fn parse(s: &str) -> Option<OpName> {
        if s.is_empty() || s.len() > MAX_OP_BYTES {
            return None;
        }
        let ok = s.split('.').all(|seg| {
            let mut b = seg.bytes();
            matches!(b.next(), Some(c) if c.is_ascii_lowercase())
                && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        });
        ok.then(|| OpName(s.to_string()))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OpName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One request.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub v: u16,
    pub id: u64,
    pub op: OpName,
    /// Always a JSON object.
    pub params: CValue,
}

/// Why a line is not a valid request; always answered with `malformed_message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Malformed {
    /// The request ID, when one could be read.
    pub id: Option<u64>,
    /// Short human-readable reason.
    pub reason: String,
}

fn malformed(id: Option<u64>, reason: &str) -> Malformed {
    Malformed { id, reason: reason.to_string() }
}

/// Longest piece of peer-supplied text echoed into a reason, in bytes.
pub const MAX_ECHO_BYTES: usize = 64;

/// Peer-supplied text made safe to echo into a log line or reply: control characters
/// are escaped and the result is cut at [`MAX_ECHO_BYTES`] with a visible marker.
pub fn printable(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let piece: String = if c.is_control() { c.escape_default().collect() } else { c.to_string() };
        if out.len() + piece.len() > MAX_ECHO_BYTES {
            out.push_str("...[truncated]");
            return format!("{out:?}");
        }
        out.push_str(&piece);
    }
    format!("{out:?}")
}

/// A non-negative integer JSON number no larger than `max`.
fn uint(v: &CValue, max: u64) -> Option<u64> {
    match v {
        CValue::Num(f) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 && *f <= max as f64 => Some(*f as u64),
        _ => None,
    }
}

/// Parses and schema-checks one request line (newline excluded).
pub fn parse_request(line: &[u8]) -> Result<Request, Malformed> {
    let value = parse_with_limits(line, MAX_MESSAGE_BYTES, MAX_DEPTH)
        .map_err(|e| malformed(None, &format!("not acceptable JSON: {}", e.code())))?;
    let CValue::Obj(mut o) = value else {
        return Err(malformed(None, "a message must be a JSON object"));
    };
    let id = o.get("id").and_then(|v| uint(v, MAX_ID));
    if !o.contains_key("id") {
        return Err(malformed(None, "missing field id"));
    }
    let Some(id) = id else {
        return Err(malformed(None, "id must be an integer from 0 to 2^53-1"));
    };
    let v = match o.remove("v") {
        None => return Err(malformed(Some(id), "missing field v")),
        Some(v) => {
            uint(&v, u64::from(u16::MAX)).ok_or_else(|| malformed(Some(id), "v must be an integer from 0 to 65535"))?
        }
    };
    let op = match o.remove("op") {
        Some(CValue::Str(s)) => {
            OpName::parse(&s).ok_or_else(|| malformed(Some(id), "op is not a valid operation name"))?
        }
        Some(_) => return Err(malformed(Some(id), "op must be a string")),
        None => return Err(malformed(Some(id), "missing field op")),
    };
    let params = match o.remove("params") {
        Some(p @ CValue::Obj(_)) => p,
        Some(_) => return Err(malformed(Some(id), "params must be an object")),
        None => return Err(malformed(Some(id), "missing field params")),
    };
    o.remove("id");
    if let Some(k) = o.keys().next() {
        return Err(malformed(Some(id), &format!("unknown field {}", printable(k))));
    }
    Ok(Request { v: v as u16, id, op, params })
}

/// The `versions` a `hello` request lists. Schema: `params = {"versions": [u16, ...]}`,
/// at most [`MAX_HELLO_VERSIONS`] entries; an empty list is valid and supports nothing.
pub fn hello_versions(req: &Request) -> Result<Vec<u16>, Malformed> {
    let bad = |r: &str| malformed(Some(req.id), r);
    let CValue::Obj(p) = &req.params else {
        return Err(bad("params must be an object"));
    };
    if let Some(k) = p.keys().find(|k| k.as_str() != "versions") {
        return Err(bad(&format!("unknown hello field {}", printable(k))));
    }
    let Some(CValue::Arr(list)) = p.get("versions") else {
        return Err(bad("hello needs params.versions, an array"));
    };
    if list.len() > MAX_HELLO_VERSIONS {
        return Err(bad("too many versions in hello"));
    }
    list.iter()
        .map(|x| {
            uint(x, u64::from(u16::MAX))
                .map(|n| n as u16)
                .ok_or_else(|| bad("versions must be integers from 0 to 65535"))
        })
        .collect()
}

/// The highest version both sides support.
pub fn negotiate(client: &[u16]) -> Option<u16> {
    PROTOCOL_VERSIONS.iter().rev().copied().find(|v| client.contains(v))
}

/// A reply body: a command result envelope or an event.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplyBody {
    /// Boxed: an envelope is much larger than an event value.
    Envelope(Box<Envelope>),
    /// An object with a string `kind` (`hello`, `error`, or an operation's own kind).
    Event(CValue),
}

/// One reply line.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub v: u16,
    /// `None` encodes as JSON `null` (the request ID could not be read).
    pub id: Option<u64>,
    pub body: ReplyBody,
}

fn num(n: u64) -> CValue {
    CValue::Num(n as f64)
}

/// The supported versions as a JSON array.
pub fn versions_value() -> CValue {
    CValue::Arr(PROTOCOL_VERSIONS.iter().map(|&v| num(u64::from(v))).collect())
}

/// An `error` event: `{"kind": "error", "code": ..., "message": ...}` plus `versions`
/// for `version_unsupported`.
pub fn error_event(code: WireError, message: &str) -> CValue {
    let mut o = BTreeMap::new();
    o.insert("kind".to_string(), CValue::Str("error".into()));
    o.insert("code".to_string(), CValue::Str(code.code().into()));
    o.insert("message".to_string(), CValue::Str(message.into()));
    if code == WireError::VersionUnsupported {
        o.insert("versions".to_string(), versions_value());
    }
    CValue::Obj(o)
}

/// The successful `hello` event: `{"kind": "hello", "version": n, "versions": [...], "instance_id": ...}`.
pub fn hello_event(version: u16, instance_id: &str) -> CValue {
    let mut o = BTreeMap::new();
    o.insert("kind".to_string(), CValue::Str("hello".into()));
    o.insert("version".to_string(), num(u64::from(version)));
    o.insert("versions".to_string(), versions_value());
    o.insert("instance_id".to_string(), CValue::Str(instance_id.into()));
    CValue::Obj(o)
}

impl Reply {
    /// An error event reply.
    pub fn error(id: Option<u64>, code: WireError, message: &str) -> Reply {
        Reply { v: PROTOCOL_VERSIONS[0], id, body: ReplyBody::Event(error_event(code, message)) }
    }

    /// Encodes as one canonical JSON line, newline included. Envelopes are validated.
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        let mut o = BTreeMap::new();
        o.insert("v".to_string(), num(u64::from(self.v)));
        o.insert("id".to_string(), self.id.filter(|&i| i <= MAX_ID).map_or(CValue::Null, num));
        match &self.body {
            ReplyBody::Envelope(e) => {
                e.validate().map_err(|_| WireError::MalformedMessage)?;
                o.insert("envelope".to_string(), e.to_value());
            }
            ReplyBody::Event(ev) => {
                o.insert("event".to_string(), ev.clone());
            }
        }
        let mut out = to_canonical_bytes(&CValue::Obj(o)).map_err(|_| WireError::MalformedMessage)?;
        out.push(b'\n');
        Ok(out)
    }

    /// Decodes one reply line (newline excluded) with the bound for operation `op`.
    pub fn decode(line: &[u8], op: &str) -> Result<Reply, WireError> {
        if line.len() > reply_limit(op) {
            return Err(WireError::MessageTooLarge);
        }
        let CValue::Obj(mut o) =
            parse_with_limits(line, reply_limit(op), fid_canon::MAX_DEPTH).map_err(|_| WireError::MalformedMessage)?
        else {
            return Err(WireError::MalformedMessage);
        };
        let v = o.remove("v").and_then(|v| uint(&v, u64::from(u16::MAX))).ok_or(WireError::MalformedMessage)?;
        let id = match o.remove("id") {
            Some(CValue::Null) => None,
            Some(x) => Some(uint(&x, MAX_ID).ok_or(WireError::MalformedMessage)?),
            None => return Err(WireError::MalformedMessage),
        };
        let body = match (o.remove("envelope"), o.remove("event")) {
            (Some(env), None) => {
                let bytes = to_canonical_bytes(&env).map_err(|_| WireError::MalformedMessage)?;
                ReplyBody::Envelope(Box::new(Envelope::decode(&bytes).map_err(|_| WireError::MalformedMessage)?))
            }
            (None, Some(ev @ CValue::Obj(_))) => ReplyBody::Event(ev),
            _ => return Err(WireError::MalformedMessage),
        };
        if !o.is_empty() {
            return Err(WireError::MalformedMessage);
        }
        Ok(Reply { v: v as u16, id, body })
    }
}

/// The server's verdict on one framed line, as the fixture corpus spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A valid request (for `hello`, with valid params too).
    Ok(String),
    /// `malformed_message`, with the request ID if one could be read.
    Malformed(Option<u64>),
    /// `message_too_large`; the connection closes, nothing after it is read.
    TooLarge,
    /// The stream ended in the middle of a line.
    Partial,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Ok(op) => write!(f, "ok:{op}"),
            Verdict::Malformed(Some(id)) => write!(f, "malformed:{id}"),
            Verdict::Malformed(None) => f.write_str("malformed:-"),
            Verdict::TooLarge => f.write_str("too_large"),
            Verdict::Partial => f.write_str("partial"),
        }
    }
}

/// Schema verdict for one complete line.
pub fn classify_line(line: &[u8]) -> Verdict {
    match parse_request(line) {
        Err(m) => Verdict::Malformed(m.id),
        Ok(req) if req.op.as_str() == HELLO_OP => match hello_versions(&req) {
            Ok(_) => Verdict::Ok(req.op.to_string()),
            Err(m) => Verdict::Malformed(m.id),
        },
        Ok(req) => Verdict::Ok(req.op.to_string()),
    }
}

/// Frames a byte stream the way a connection does and classifies each line, stopping at
/// the first oversized line. Pure: this is the `ctrl_frame` fuzz target's entry point.
pub fn classify_stream(input: &[u8], limit: usize) -> Vec<Verdict> {
    let mut reader = LineReader::new(input);
    let mut out = Vec::new();
    loop {
        match reader.next_line(limit) {
            Ok(Frame::Line(line)) => out.push(classify_line(&line)),
            Ok(Frame::TooLarge) => {
                out.push(Verdict::TooLarge);
                break;
            }
            Ok(Frame::PartialEof) => {
                out.push(Verdict::Partial);
                break;
            }
            Ok(Frame::Eof) | Err(_) => break,
        }
    }
    out
}
