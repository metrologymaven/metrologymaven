//! The MCP stdio client: line-delimited JSON-RPC 2.0, every message RFC 8785 canonical JSON.
//!
//! Message order: `initialize` (id 1), `notifications/initialized`, `tools/list` (following
//! `nextCursor`, at most [`MAX_LIST_PAGES`] pages), then one `tools/call` per MCP attempt in
//! plan order. Request IDs count up from 1. The only server values copied into requests are a
//! `tools/list` cursor and, for self-approval, the last `request_id` a reply returned (the
//! forbidden family's direct W3, X1 and D1 calls file pending requests on a real server, so
//! the adoption family's `approve*` attempts then approve that request instead of a seeded
//! ID). For a given task, manifest and server that answers the same way, the request stream is
//! byte-identical across runs (REQ-AGENT.REDTEAM.SCRIPT-004); a server whose cursors or
//! request IDs vary makes those values vary too. The only use of the tool list is to name the
//! dispatcher: the first listed tool, by name, ending in `_invoke_readonly`, else
//! [`DEFAULT_DISPATCHER`].
//!
//! The log (stderr in the binary) gets every planned attempt, then one line per executed
//! attempt `{"outcome": ..., "seq": n}` with outcome `ok`, `error` (JSON-RPC error or
//! `isError: true`; refusals are expected), `no_reply` (the server closed, or sent
//! [`MAX_SKIPPED_MESSAGES`] other messages without answering), `not_sent`, `spawn_failed` or,
//! for CLI attempts, `exit_ok` / `exit_error`.

use crate::plan::{Attempt, Family, Route, TaskView};
use crate::AdversaryError;
use crate::{build_plan, Manifest};
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};

/// Dispatcher tool name used when the server lists none (AUTO.MCP.DISC `tools.invoke_readonly@1`).
pub const DEFAULT_DISPATCHER: &str = "tools_invoke_readonly";
/// MCP protocol revision announced in `initialize`.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Adversary version announced in `initialize` (bumped only when the request stream changes).
pub const ADVERSARY_VERSION: &str = "1";
/// Largest accepted server message, in bytes (4 MiB, as REQ-AUTO.MCP.SESS-006).
pub const MAX_MESSAGE_BYTES: u64 = 4 * 1024 * 1024;
/// Most `tools/list` pages followed.
pub const MAX_LIST_PAGES: usize = 64;
/// Most unrelated server messages skipped while waiting for one reply.
pub const MAX_SKIPPED_MESSAGES: usize = 1024;
/// Deepest nesting searched for a `request_id` in a reply.
const MAX_REPLY_DEPTH: usize = 8;
/// Largest embedded JSON text searched for a `request_id`.
const MAX_REPLY_TEXT: usize = 64 * 1024;

/// Runs a CLI attempt (shell variants): `argv` without the program, `stdin` the parameters.
pub trait CliRunner {
    /// Returns the outcome word for the log.
    fn run(&mut self, argv: &[String], stdin: &[u8]) -> &'static str;
}

/// Runs the `fid` CLI as a child process.
pub struct ProcessCli {
    pub program: String,
}

impl CliRunner for ProcessCli {
    fn run(&mut self, argv: &[String], stdin: &[u8]) -> &'static str {
        use std::process::{Command, Stdio};
        let child = Command::new(&self.program)
            .args(argv)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            return "spawn_failed";
        };
        if let Some(mut input) = child.stdin.take() {
            // A child that exits without reading its input is not an error of the adversary.
            let _ = input.write_all(stdin);
        }
        match child.wait() {
            Ok(s) if s.success() => "exit_ok",
            _ => "exit_error",
        }
    }
}

/// Counts of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub planned: usize,
    pub sent: usize,
    pub cli: usize,
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

/// Canonical bytes of a value plus a newline.
pub fn line(v: &CValue) -> Result<Vec<u8>, AdversaryError> {
    let mut b = fid_canon::to_canonical_bytes(v).map_err(|e| AdversaryError::Io(format!("canonical JSON: {e}")))?;
    b.push(b'\n');
    Ok(b)
}

/// The JSON-RPC request of an MCP attempt; `None` for a CLI attempt.
pub fn call_request(id: u64, a: &Attempt, dispatcher: &str) -> Option<CValue> {
    let params = match a.route {
        Route::Cli => return None,
        Route::Dispatcher => obj(vec![
            ("arguments", obj(vec![("args", CValue::Obj(a.args.clone())), ("command", s(&a.command.id()))])),
            ("name", s(dispatcher)),
        ]),
        Route::Direct | Route::InjectedFields => {
            obj(vec![("arguments", CValue::Obj(a.args.clone())), ("name", s(&a.command.tool_name()))])
        }
    };
    Some(request(id, "tools/call", params))
}

fn request(id: u64, method: &str, params: CValue) -> CValue {
    obj(vec![("id", CValue::Num(id as f64)), ("jsonrpc", s("2.0")), ("method", s(method)), ("params", params)])
}

/// The CLI argv of an attempt: `<domain> <verb-object> --json --params -`.
pub fn cli_argv(a: &Attempt) -> Vec<String> {
    let mut v = a.command.cli_words();
    v.extend(["--json", "--params", "-"].iter().map(|w| w.to_string()));
    v
}

enum Reply {
    Ok(CValue),
    Error,
    Closed,
    /// Too many unrelated messages; the session stays open.
    Skipped,
}

/// The first string `request_id` in a reply result, searching objects, arrays and JSON text
/// in `text` fields (MCP `content` items), bounded in depth and size.
pub fn find_request_id(v: &CValue) -> Option<String> {
    fn walk(v: &CValue, depth: usize) -> Option<String> {
        if depth > MAX_REPLY_DEPTH {
            return None;
        }
        match v {
            CValue::Obj(m) => {
                if let Some(CValue::Str(id)) = m.get("request_id") {
                    return Some(id.clone());
                }
                for (k, x) in m {
                    if let (true, CValue::Str(text)) = (k == "text", x) {
                        if text.len() <= MAX_REPLY_TEXT {
                            if let Ok(inner) = fid_canon::parse_with_limits(text.as_bytes(), MAX_REPLY_TEXT, 16) {
                                if let Some(id) = walk(&inner, depth + 1) {
                                    return Some(id);
                                }
                            }
                        }
                    } else if let Some(id) = walk(x, depth + 1) {
                        return Some(id);
                    }
                }
                None
            }
            CValue::Arr(a) => a.iter().find_map(|x| walk(x, depth + 1)),
            _ => None,
        }
    }
    walk(v, 0)
}

/// Whether an attempt approves a request (adoption family, verb starting with `approve`).
fn is_approval(a: &Attempt) -> bool {
    a.family == Family::Adoption && a.command.name.rsplit('.').next().is_some_and(|v| v.starts_with("approve"))
}

/// The attempt with every string argument named like a request replaced by `request_id`.
fn with_request_id(a: &Attempt, request_id: &str) -> Attempt {
    let mut b = a.clone();
    for (k, v) in b.args.iter_mut() {
        if k.contains("request") && matches!(v, CValue::Str(_)) {
            *v = CValue::Str(request_id.to_string());
        }
    }
    b
}

struct Client<R, W> {
    reader: R,
    writer: W,
    next_id: u64,
    open: bool,
}

impl<R: BufRead, W: Write> Client<R, W> {
    fn send(&mut self, v: &CValue) -> Result<bool, AdversaryError> {
        let bytes = line(v)?;
        if self.writer.write_all(&bytes).and_then(|()| self.writer.flush()).is_err() {
            self.open = false;
        }
        Ok(self.open)
    }

    /// Sends a request and waits for the response with its ID, skipping other messages.
    fn call(&mut self, method: &str, params: CValue) -> Result<Reply, AdversaryError> {
        let id = self.next_id;
        self.next_id += 1;
        if !self.open || !self.send(&request(id, method, params))? {
            return Ok(Reply::Closed);
        }
        self.await_reply(id)
    }

    fn await_reply(&mut self, id: u64) -> Result<Reply, AdversaryError> {
        for _ in 0..=MAX_SKIPPED_MESSAGES {
            let mut buf = Vec::new();
            let n = (&mut self.reader)
                .take(MAX_MESSAGE_BYTES + 1)
                .read_until(b'\n', &mut buf)
                .map_err(|e| AdversaryError::Io(format!("reading from server: {e}")))?;
            if n == 0 {
                self.open = false;
                return Ok(Reply::Closed);
            }
            if buf.len() as u64 > MAX_MESSAGE_BYTES {
                return Err(AdversaryError::Io("server message over 4 MiB".into()));
            }
            let Ok(CValue::Obj(m)) = fid_canon::parse(&buf) else {
                continue; // not a JSON-RPC object: ignored
            };
            if m.get("id") != Some(&CValue::Num(id as f64)) || m.contains_key("method") {
                continue; // a notification, a server request or another response
            }
            if m.contains_key("error") {
                return Ok(Reply::Error);
            }
            let result = m.get("result").cloned().unwrap_or(CValue::Null);
            if let CValue::Obj(r) = &result {
                if r.get("isError") == Some(&CValue::Bool(true)) {
                    return Ok(Reply::Error);
                }
            }
            return Ok(Reply::Ok(result));
        }
        Ok(Reply::Skipped)
    }
}

fn outcome_line(seq: usize, outcome: &str) -> CValue {
    obj(vec![("outcome", s(outcome)), ("seq", CValue::Num(seq as f64))])
}

/// Runs the adversary for one task over an MCP session on `reader`/`writer`.
pub fn run<R: BufRead, W: Write, L: Write>(
    task: &TaskView,
    manifest: &Manifest,
    reader: R,
    writer: W,
    log: &mut L,
    cli: &mut dyn CliRunner,
) -> Result<Summary, AdversaryError> {
    let mut c = Client { reader, writer, next_id: 1, open: true };
    let init = obj(vec![
        ("capabilities", obj(vec![])),
        ("clientInfo", obj(vec![("name", s("fid-eval-adversary")), ("version", s(ADVERSARY_VERSION))])),
        ("protocolVersion", s(PROTOCOL_VERSION)),
    ]);
    let mut tools: Vec<String> = Vec::new();
    if let Reply::Ok(_) = c.call("initialize", init)? {
        c.send(&obj(vec![("jsonrpc", s("2.0")), ("method", s("notifications/initialized"))]))?;
        let mut params = obj(vec![]);
        for _ in 0..MAX_LIST_PAGES {
            let Reply::Ok(CValue::Obj(r)) = c.call("tools/list", params)? else {
                break;
            };
            if let Some(CValue::Arr(list)) = r.get("tools") {
                for t in list {
                    if let CValue::Obj(t) = t {
                        if let Some(CValue::Str(n)) = t.get("name") {
                            tools.push(n.clone());
                        }
                    }
                }
            }
            match r.get("nextCursor") {
                Some(CValue::Str(cur)) => params = obj(vec![("cursor", s(cur))]),
                _ => break,
            }
        }
    }
    tools.sort();
    let dispatcher =
        tools.iter().find(|n| n.ends_with("_invoke_readonly")).map(String::as_str).unwrap_or(DEFAULT_DISPATCHER);

    let plan = build_plan(task, manifest);
    let mut sum = Summary { planned: plan.len(), ..Summary::default() };
    let io = |e: std::io::Error| AdversaryError::Io(format!("writing log: {e}"));
    for a in &plan {
        log.write_all(&line(&a.to_value())?).map_err(io)?;
    }
    let mut last_request: Option<String> = None;
    for planned in &plan {
        let approved;
        let a = match &last_request {
            Some(rid) if is_approval(planned) => {
                approved = with_request_id(planned, rid);
                &approved
            }
            _ => planned,
        };
        let outcome = if a.route == Route::Cli {
            sum.cli += 1;
            let stdin = line(&CValue::Obj(a.args.clone()))?;
            cli.run(&cli_argv(a), &stdin)
        } else if !c.open {
            "not_sent"
        } else {
            let id = c.next_id;
            c.next_id += 1;
            let sent = match call_request(id, a, dispatcher) {
                Some(req) => c.send(&req)?,
                None => false,
            };
            if sent {
                sum.sent += 1;
                match c.await_reply(id)? {
                    Reply::Ok(result) => {
                        if let Some(rid) = find_request_id(&result) {
                            last_request = Some(rid);
                        }
                        "ok"
                    }
                    Reply::Error => "error",
                    Reply::Closed | Reply::Skipped => "no_reply",
                }
            } else {
                "no_reply"
            }
        };
        log.write_all(&line(&outcome_line(a.seq, outcome))?).map_err(io)?;
    }
    log.flush().map_err(io)?;
    Ok(sum)
}
