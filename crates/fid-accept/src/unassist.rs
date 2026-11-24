//! Unassisted-run principal check over golden journals and Activity Logs
//! (spec node `FLOW.RUNNER.UNASSIST`).
//!
//! Proves that an acceptance package declared unassisted had no agent acting anywhere in it:
//! - No Record Journal entry with principal kind `agent` (or acting on behalf of an agent),
//!   in a session opened over an MCP channel (including after a principal upgrade), in an
//!   agent-opened session, or targeting an `agent/` branch (REQ-FLOW.RUNNER.UNASSIST-001).
//! - No Activity Log entry attributed to an agent principal (REQ-FLOW.RUNNER.UNASSIST-002).
//! - Every entry missing or carrying a malformed principal reports `UNATTRIBUTED` rather than
//!   assuming a human (REQ-FLOW.RUNNER.UNASSIST-003).
//! - Each [`Violation`] reports the entry index, stream, command ID, principal and violation
//!   code (REQ-FLOW.RUNNER.UNASSIST-004).
//!
//! Streaming, bounded and D0: entries are read one JSON line at a time without holding the
//! journal in memory.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Cursor};
use std::path::PathBuf;

use fid_canon::{parse_with_limits, CValue};

/// Maximum accepted length of a single JSON Lines entry, in bytes (1 MiB).
pub const MAX_LINE_BYTES: usize = 1 << 20;
/// Maximum JSON nesting depth for a single journal header line.
pub const MAX_ENTRY_DEPTH: usize = 16;
/// Maximum entries scanned per stream in a single call.
pub const MAX_ENTRIES: usize = 1_000_000;

/// Known valid principal kinds (CONSTITUTION §3, D-012).
const PRINCIPAL_KINDS: [&str; 6] = ["human", "agent", "script", "service", "device", "system"];

/// Which journal stream a [`Violation`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stream {
    /// The hash-chained Record Journal.
    Record,
    /// The local Activity Log.
    Activity,
}

impl Stream {
    /// Stable wire name (`record` or `activity`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Stream::Record => "record",
            Stream::Activity => "activity",
        }
    }
}

impl fmt::Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Machine-readable reason why an entry violates the unassisted-run rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViolationCode {
    /// Entry principal is `agent` (or acting on behalf of an agent).
    AgentPrincipal,
    /// Entry ran in a session opened over an MCP channel (or carried an MCP channel tag).
    McpChannel,
    /// Entry targets an agent branch (`agent/...`).
    AgentBranch,
    /// Entry ran inside a session opened by an agent principal.
    AgentSession,
    /// Entry lacks a valid principal attribution (REQ-FLOW.RUNNER.UNASSIST-003).
    Unattributed,
}

impl ViolationCode {
    /// Stable machine-readable code string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ViolationCode::AgentPrincipal => "AGENT_PRINCIPAL",
            ViolationCode::McpChannel => "MCP_CHANNEL",
            ViolationCode::AgentBranch => "AGENT_BRANCH",
            ViolationCode::AgentSession => "AGENT_SESSION",
            ViolationCode::Unattributed => "UNATTRIBUTED",
        }
    }
}

impl fmt::Display for ViolationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One unassisted-run violation (REQ-FLOW.RUNNER.UNASSIST-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// 0-based entry index within its stream.
    pub index: usize,
    /// Stream (`record` or `activity`).
    pub stream: Stream,
    /// Command ID (`name@major`), entry kind for non-command entries, or `<none>`.
    pub command_id: String,
    /// Principal string (e.g. `agent`, `agent:<id>`, `human`, or `<none>`).
    pub principal: String,
    /// Machine-readable violation code.
    pub code: ViolationCode,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} stream={} index={} command={} principal={}",
            self.code, self.stream, self.index, self.command_id, self.principal
        )
    }
}

/// Error reading or parsing a journal stream (`UNASSIST-READ`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnassistError {
    /// Underlying read, framing, UTF-8, or JSON parse failure.
    Read(String),
}

impl UnassistError {
    /// Stable machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            UnassistError::Read(_) => "UNASSIST-READ",
        }
    }
}

impl fmt::Display for UnassistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnassistError::Read(msg) => write!(f, "UNASSIST-READ: {msg}"),
        }
    }
}

impl std::error::Error for UnassistError {}

#[derive(Debug, Clone)]
enum StreamSource {
    Path(PathBuf),
    Bytes(Vec<u8>),
}

/// Input reader over a Record Journal stream and optional Activity Log stream.
#[derive(Debug, Clone)]
pub struct JournalReader {
    record: StreamSource,
    activity: Option<StreamSource>,
}

impl JournalReader {
    /// Reads a Record Journal JSON Lines file (no Activity Log).
    #[must_use]
    pub fn from_path(record_path: impl Into<PathBuf>) -> Self {
        Self { record: StreamSource::Path(record_path.into()), activity: None }
    }

    /// Reads a Record Journal JSON Lines file and an optional Activity Log JSON Lines file.
    #[must_use]
    pub fn from_paths(record_path: impl Into<PathBuf>, activity_path: Option<PathBuf>) -> Self {
        Self { record: StreamSource::Path(record_path.into()), activity: activity_path.map(StreamSource::Path) }
    }

    /// Reads a Record Journal from in-memory JSON Lines bytes.
    #[must_use]
    pub fn from_bytes(record_jsonl: Vec<u8>) -> Self {
        Self { record: StreamSource::Bytes(record_jsonl), activity: None }
    }

    /// Reads a Record Journal and optional Activity Log from in-memory JSON Lines bytes.
    #[must_use]
    pub fn from_bytes_with_activity(record_jsonl: Vec<u8>, activity_jsonl: Option<Vec<u8>>) -> Self {
        Self { record: StreamSource::Bytes(record_jsonl), activity: activity_jsonl.map(StreamSource::Bytes) }
    }
}

/// Checks a golden journal (and optional Activity Log) for any agent attribution or
/// unattributed entry (REQ-FLOW.RUNNER.UNASSIST-001..004).
pub fn check_unassisted(journal: &JournalReader) -> Result<Vec<Violation>, UnassistError> {
    let mut violations = Vec::new();
    scan_source(&journal.record, Stream::Record, &mut violations)?;
    if let Some(act) = &journal.activity {
        scan_source(act, Stream::Activity, &mut violations)?;
    }
    Ok(violations)
}

fn scan_source(source: &StreamSource, default_stream: Stream, out: &mut Vec<Violation>) -> Result<(), UnassistError> {
    match source {
        StreamSource::Path(path) => {
            let meta = fs::symlink_metadata(path)
                .map_err(|_| UnassistError::Read(format!("cannot stat {}", path.display())))?;
            if meta.file_type().is_symlink() {
                return Err(UnassistError::Read(format!("symlink refused: {}", path.display())));
            }
            if !meta.file_type().is_file() {
                return Err(UnassistError::Read(format!("not a regular file: {}", path.display())));
            }
            let file = File::open(path).map_err(|_| UnassistError::Read(format!("cannot open {}", path.display())))?;
            scan_reader(BufReader::new(file), default_stream, out)
        }
        StreamSource::Bytes(bytes) => scan_reader(BufReader::new(Cursor::new(bytes.as_slice())), default_stream, out),
    }
}

#[derive(Default)]
struct SessionState {
    mcp_channel: bool,
    agent_session: bool,
}

fn scan_reader<R: BufRead>(
    mut reader: R,
    default_stream: Stream,
    out: &mut Vec<Violation>,
) -> Result<(), UnassistError> {
    let mut session = SessionState::default();
    let mut buf = Vec::new();
    let mut index = 0usize;

    loop {
        buf.clear();
        let n = read_bounded_line(&mut reader, &mut buf, MAX_LINE_BYTES)?;
        if n == 0 {
            break;
        }
        if index >= MAX_ENTRIES {
            return Err(UnassistError::Read(format!("stream exceeds {MAX_ENTRIES} entries")));
        }

        // Strip trailing LF (and optional CR before LF).
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        if buf.is_empty() {
            return Err(UnassistError::Read(format!("blank line at entry index {index} in stream {default_stream}")));
        }

        let val = parse_with_limits(&buf, MAX_LINE_BYTES, MAX_ENTRY_DEPTH).map_err(|e| {
            UnassistError::Read(format!("invalid JSON at entry index {index} in stream {default_stream}: {e}"))
        })?;
        let CValue::Obj(obj) = val else {
            return Err(UnassistError::Read(format!(
                "entry at index {index} in stream {default_stream} must be a JSON object"
            )));
        };

        let entry_stream = parse_stream(&obj, default_stream);
        let command_id = extract_command_id(&obj);
        let principal = extract_principal(&obj);

        let is_session_header =
            matches!(obj.get("kind"), Some(CValue::Str(k)) if k == "session_header") || obj.contains_key("session");
        let is_upgrade =
            matches!(obj.get("kind"), Some(CValue::Str(k)) if k == "principal_upgrade") || obj.contains_key("upgrade");

        let entry_channel_mcp = entry_has_mcp_channel(&obj);
        let branch_is_agent = entry_targets_agent_branch(&obj);

        if entry_stream == Stream::Record {
            if is_session_header {
                session.mcp_channel = entry_channel_mcp;
                session.agent_session = principal.is_agent();
            } else if is_upgrade {
                // An MCP-opened session stays MCP-opened even after a human upgrade confirmation.
                if entry_channel_mcp {
                    session.mcp_channel = true;
                }
                if principal.is_agent() || upgrade_targets_agent(&obj) {
                    session.agent_session = true;
                }
            }
        }

        let violation_code = match &principal {
            PrincipalInfo::Unattributed => Some(ViolationCode::Unattributed),
            PrincipalInfo::Known { is_agent: true, .. } => Some(ViolationCode::AgentPrincipal),
            PrincipalInfo::Known { is_agent: false, .. } => {
                if entry_channel_mcp || (entry_stream == Stream::Record && session.mcp_channel) {
                    Some(ViolationCode::McpChannel)
                } else if branch_is_agent {
                    Some(ViolationCode::AgentBranch)
                } else if entry_stream == Stream::Record && session.agent_session {
                    Some(ViolationCode::AgentSession)
                } else {
                    None
                }
            }
        };

        if let Some(code) = violation_code {
            out.push(Violation { index, stream: entry_stream, command_id, principal: principal.label(), code });
        }

        index += 1;
    }

    Ok(())
}

fn read_bounded_line<R: BufRead>(reader: &mut R, out: &mut Vec<u8>, max_bytes: usize) -> Result<usize, UnassistError> {
    let mut total = 0usize;
    loop {
        let avail = reader.fill_buf().map_err(|_| UnassistError::Read("I/O error reading journal".to_string()))?;
        if avail.is_empty() {
            return Ok(total);
        }
        let (take, done) = match avail.iter().position(|&b| b == b'\n') {
            Some(pos) => (pos + 1, true),
            None => (avail.len(), false),
        };
        if total + take > max_bytes {
            return Err(UnassistError::Read(format!("journal line exceeds {max_bytes} bytes")));
        }
        out.extend_from_slice(&avail[..take]);
        reader.consume(take);
        total += take;
        if done {
            return Ok(total);
        }
    }
}

enum PrincipalInfo {
    Unattributed,
    Known { label: String, is_agent: bool },
}

impl PrincipalInfo {
    fn is_agent(&self) -> bool {
        matches!(self, PrincipalInfo::Known { is_agent: true, .. })
    }

    fn label(&self) -> String {
        match self {
            PrincipalInfo::Unattributed => "<none>".to_string(),
            PrincipalInfo::Known { label, .. } => label.clone(),
        }
    }
}

fn is_valid_principal_kind(s: &str) -> bool {
    PRINCIPAL_KINDS.contains(&s)
}

fn extract_principal(obj: &BTreeMap<String, CValue>) -> PrincipalInfo {
    match obj.get("principal") {
        None | Some(CValue::Null) => PrincipalInfo::Unattributed,
        Some(CValue::Str(s)) => {
            if is_valid_principal_kind(s) {
                PrincipalInfo::Known { label: s.clone(), is_agent: s == "agent" }
            } else {
                PrincipalInfo::Unattributed
            }
        }
        Some(CValue::Obj(p)) => {
            let Some(CValue::Str(kind)) = p.get("kind") else {
                return PrincipalInfo::Unattributed;
            };
            if !is_valid_principal_kind(kind) {
                return PrincipalInfo::Unattributed;
            }
            let obo_agent = match p.get("on_behalf_of_kind") {
                Some(CValue::Str(k)) => k == "agent",
                _ => false,
            } || match p.get("on_behalf_of") {
                Some(CValue::Str(k)) => k == "agent",
                Some(CValue::Obj(obo)) => matches!(obo.get("kind"), Some(CValue::Str(k)) if k == "agent"),
                _ => false,
            };
            let is_agent = kind == "agent" || obo_agent;
            let suffix = match p.get("id").or_else(|| p.get("commit")) {
                Some(CValue::Str(id)) if !id.is_empty() => Some(id.as_str()),
                _ => None,
            };
            let label = match suffix {
                Some(id) => format!("{kind}:{id}"),
                None => kind.clone(),
            };
            PrincipalInfo::Known { label, is_agent }
        }
        _ => PrincipalInfo::Unattributed,
    }
}

fn parse_stream(obj: &BTreeMap<String, CValue>, default_stream: Stream) -> Stream {
    match obj.get("stream") {
        Some(CValue::Str(s)) if s == "activity" => Stream::Activity,
        Some(CValue::Str(s)) if s == "record" => Stream::Record,
        _ => default_stream,
    }
}

fn extract_command_id(obj: &BTreeMap<String, CValue>) -> String {
    match obj.get("command") {
        Some(CValue::Str(s)) if !s.is_empty() => return s.clone(),
        Some(CValue::Obj(cmd)) => {
            if let Some(CValue::Str(id)) = cmd.get("id") {
                if let Some(CValue::Num(major)) = cmd.get("major") {
                    if major.is_finite() && *major >= 0.0 && major.fract() == 0.0 {
                        return format!("{id}@{}", *major as u64);
                    }
                }
                return id.clone();
            }
        }
        _ => {}
    }
    if let Some(CValue::Str(cid)) = obj.get("command_id") {
        if !cid.is_empty() {
            return cid.clone();
        }
    }
    if let Some(CValue::Str(kind)) = obj.get("kind") {
        if !kind.is_empty() {
            return kind.clone();
        }
    }
    "<none>".to_string()
}

fn is_mcp_channel_name(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower == "mcp" || lower.starts_with("mcp_") || lower.starts_with("mcp-") || lower.starts_with("mcp/")
}

fn entry_has_mcp_channel(obj: &BTreeMap<String, CValue>) -> bool {
    if let Some(CValue::Str(ch)) = obj.get("channel") {
        if is_mcp_channel_name(ch) {
            return true;
        }
    }
    if let Some(CValue::Obj(sess)) = obj.get("session") {
        if let Some(CValue::Str(ch)) = sess.get("channel") {
            if is_mcp_channel_name(ch) {
                return true;
            }
        }
    }
    false
}

fn entry_targets_agent_branch(obj: &BTreeMap<String, CValue>) -> bool {
    match obj.get("branch") {
        Some(CValue::Str(b)) => b == "agent" || b.starts_with("agent/"),
        _ => false,
    }
}

fn upgrade_targets_agent(obj: &BTreeMap<String, CValue>) -> bool {
    let Some(CValue::Obj(up)) = obj.get("upgrade") else {
        return false;
    };
    matches!(up.get("to_kind").or_else(|| up.get("kind")), Some(CValue::Str(k)) if k == "agent")
}
