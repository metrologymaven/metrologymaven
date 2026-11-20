//! The tool-call adapter (REQ-005): one MCP tool call becomes one bus invocation on channel
//! [`CHANNEL`] for the session's agent, and the bus envelope comes back unchanged.
//!
//! The person/agent rule holds by construction:
//!
//! - **Only generated tools are callable.** [`ToolTable`] is built only from a projection
//!   through [`crate::tools::tools`], so a command with no tool (M1, S1, A0, `never`, not
//!   exposed on `mcp`) has no entry and a call to it is refused before the bus is reached.
//! - **The principal comes only from the channel.** A [`BusCall`] carries the channel, the
//!   session and the command, and has no principal, role or approval field; the bus derives
//!   the principal from the channel (`CORE.BUS.PRIN`: an MCP channel is an `agent`). Tool
//!   arguments go to the bus as the command's parameters and nothing else, so a `principal`
//!   or `approval_of` member in them is just an unknown parameter the bus refuses.
//! - **Request-only tools only file, and the bus is the only gate.** Whether a call runs or
//!   becomes a pending request is decided by the bus alone (`CORE.BUS.POL.PROJ`, behind every
//!   surface); the adapter cannot stop a command the bus chose to run. After the call it
//!   accepts for a request-only tool only an envelope that shows nothing ran (`request_filed`,
//!   `failed`, `not_run`, `no_reply`, `cancelled`). That check *detects*, it does not
//!   *prevent*: any other outcome means the bus and the projection disagree, the action may
//!   already have happened, and the adapter reports a [`Violation`] at error severity to the
//!   [`AuditSink`] (for the session layer to log and keep for audit) and answers the agent
//!   `internal/invariant_violated` instead of passing the result on as a success. The same
//!   holds for an invalid envelope or one for another command.
//!
//! Every refusal is a code and reason of the bus error catalogue (`bus-errors.json`, v3):
//! [`Refusal::code`] and [`Refusal::reason`].

use crate::tools::{tools, McpError, ToolKind};
use fid_canon::CValue;
use fid_surface::{Envelope, ErrorCode, Outcome, Projection};
use fid_types::CommandId;
use std::collections::BTreeMap;
use std::fmt;

/// The bus channel every MCP tool call uses (`ChannelKind::Mcp` on the bus).
pub const CHANNEL: &str = "mcp";

/// The principal kind the bus derives for [`CHANNEL`]; an MCP session has no other.
pub const PRINCIPAL_KIND: &str = "agent";

/// One MCP server session as the adapter sees it: an opaque label the session layer
/// (`AUTO.MCP.SESS`) binds to its authenticated agent. It carries no principal of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSession {
    label: String,
}

impl AgentSession {
    /// A session with the session layer's label.
    pub fn new(label: &str) -> AgentSession {
        AgentSession { label: label.to_string() }
    }

    /// The label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Always [`PRINCIPAL_KIND`].
    pub fn principal_kind(&self) -> &'static str {
        PRINCIPAL_KIND
    }
}

/// What the adapter hands the bus for one call. There is deliberately no principal and no
/// approval in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BusCall<'a> {
    /// Always [`CHANNEL`].
    pub channel: &'static str,
    pub session: &'a AgentSession,
    pub command: &'a CommandId,
    /// The tool arguments, unchanged.
    pub params: &'a CValue,
}

/// The bus as the adapter reaches it. Implemented by the session layer over the real bus,
/// and by test harnesses.
pub trait McpBus {
    /// Runs the full bus pipeline for `call` and returns its envelope.
    fn invoke(&mut self, call: &BusCall<'_>) -> Envelope;
}

/// The callable tools: tool name to command and kind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolTable {
    by_name: BTreeMap<String, (CommandId, ToolKind)>,
}

impl ToolTable {
    /// The tools of `p`, exactly as [`crate::tools::emit`] lists them.
    pub fn from_projection(p: &Projection) -> Result<ToolTable, McpError> {
        let by_name = tools(p)?.into_iter().map(|t| (t.name, (t.command, t.kind))).collect();
        Ok(ToolTable { by_name })
    }

    /// The command and kind of a tool.
    pub fn get(&self, name: &str) -> Option<(&CommandId, ToolKind)> {
        self.by_name.get(name).map(|(c, k)| (c, *k))
    }

    /// Tool names, in byte order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }

    /// Number of tools.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// No tools.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

/// Severity of an audit event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The bus and the projection disagree, or the bus broke the envelope contract.
    Error,
}

/// An invariant the bus broke on one call, reported for audit. The action the bus took may
/// already have happened; this records it, it does not undo it.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    pub severity: Severity,
    /// The session's label.
    pub session: String,
    pub tool: String,
    pub command: CommandId,
    /// The envelope the bus returned, unchanged.
    pub envelope: Envelope,
    pub detail: String,
}

/// Where the adapter reports [`Violation`]s. The session layer (`AUTO.MCP.SESS`) logs them at
/// error level and keeps them for audit.
pub trait AuditSink {
    /// Records one violation.
    fn violation(&mut self, v: Violation);
}

/// Why the adapter refused a call. Each maps to one code and reason of the bus error
/// catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No generated tool has this name: `not_found/unknown_command`.
    UnknownTool { detail: String },
    /// The arguments are not a JSON object: `validation/schema`.
    BadArguments { detail: String },
    /// The bus's envelope is invalid, names another command, or shows that a request-only
    /// command ran: `internal/invariant_violated`.
    InvariantViolated { detail: String },
}

impl Refusal {
    /// Every (code, reason) pair a refusal can carry.
    pub const PAIRS: [(&'static str, &'static str); 3] =
        [("not_found", "unknown_command"), ("validation", "schema"), ("internal", "invariant_violated")];

    /// The bus error code.
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::UnknownTool { .. } => "not_found",
            Refusal::BadArguments { .. } => "validation",
            Refusal::InvariantViolated { .. } => "internal",
        }
    }

    /// The bus error reason.
    pub fn reason(&self) -> &'static str {
        match self {
            Refusal::UnknownTool { .. } => "unknown_command",
            Refusal::BadArguments { .. } => "schema",
            Refusal::InvariantViolated { .. } => "invariant_violated",
        }
    }

    /// The envelope error code of the same failure (AUTO.SURF.ENV).
    pub fn envelope_code(&self) -> ErrorCode {
        match self {
            Refusal::UnknownTool { .. } => ErrorCode::NotFound,
            Refusal::BadArguments { .. } => ErrorCode::ValidationError,
            Refusal::InvariantViolated { .. } => ErrorCode::InternalError,
        }
    }

    /// The detail text.
    pub fn detail(&self) -> &str {
        match self {
            Refusal::UnknownTool { detail }
            | Refusal::BadArguments { detail }
            | Refusal::InvariantViolated { detail } => detail,
        }
    }

    /// `{code, reason, detail}`, the shape of a serialised bus error.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        o.insert("reason".to_string(), CValue::Str(self.reason().to_string()));
        o.insert("detail".to_string(), CValue::Str(self.detail().to_string()));
        CValue::Obj(o)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}: {}", self.code(), self.reason(), self.detail())
    }
}

impl std::error::Error for Refusal {}

/// The tool name, echoed only when it has the shape of a generated name (it is caller input).
fn echo_name(name: &str) -> String {
    let ok = !name.is_empty()
        && name.len() <= fid_surface::MCP_NAME_MAX
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        format!("no tool {name}")
    } else {
        "no tool with this name".to_string()
    }
}

/// Whether an outcome shows that nothing ran.
fn nothing_ran(outcome: &Outcome) -> bool {
    match outcome {
        Outcome::RequestFiled | Outcome::Failed | Outcome::NotRun | Outcome::NoReply | Outcome::Cancelled => true,
        Outcome::Committed | Outcome::Computed | Outcome::DryRun | Outcome::Accepted | Outcome::Unknown(_) => false,
    }
}

/// Calls tool `name` with `args` for `session` through `bus` (REQ-005) and returns the bus
/// envelope. Refused before the bus when the tool is unknown or the arguments are not an
/// object. When the envelope breaks an invariant (see the module docs) the call is reported to
/// `audit` and refused as `internal/invariant_violated`.
pub fn call_tool(
    table: &ToolTable,
    session: &AgentSession,
    bus: &mut dyn McpBus,
    audit: &mut dyn AuditSink,
    name: &str,
    args: &CValue,
) -> Result<Envelope, Refusal> {
    let Some((command, kind)) = table.get(name) else {
        return Err(Refusal::UnknownTool { detail: echo_name(name) });
    };
    if !matches!(args, CValue::Obj(_)) {
        return Err(Refusal::BadArguments { detail: "tool arguments must be a JSON object".to_string() });
    }
    let env = bus.invoke(&BusCall { channel: CHANNEL, session, command, params: args });
    let broken = if let Err(e) = env.validate() {
        Some(format!("the bus returned an invalid envelope: {}", e.code()))
    } else if env.command_id != command.as_str() {
        Some(format!("the bus answered for another command than {}", command.as_str()))
    } else if kind == ToolKind::RequestOnly && !nothing_ran(&env.outcome) {
        Some(format!("{} is request-only on MCP, but the bus reported {}", command.as_str(), env.outcome.as_str()))
    } else {
        None
    };
    match broken {
        None => Ok(env),
        Some(detail) => {
            audit.violation(Violation {
                severity: Severity::Error,
                session: session.label().to_string(),
                tool: name.to_string(),
                command: command.clone(),
                envelope: env,
                detail: detail.clone(),
            });
            Err(Refusal::InvariantViolated { detail })
        }
    }
}
