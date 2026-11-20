//! The MCP surface of Metrology Maven. Spec node `AUTO.MCP.PROJ`.
//!
//! Three parts, all pure except the call into the bus:
//!
//! - [`tools`]: the MCP emitter. It turns each command of the `fid-surface` projection into
//!   at most one [`McpTool`] (name as `AUTO.SURF.PROJ` derives it, description from the
//!   registry help text, JSON Schema input and output), lists no tool for an M1, S1 or A0
//!   command or for one not exposed on `mcp`, marks W3, X1 and D1 tools (and `human_only`
//!   commands) request-only, and fails generation with `motion_tool_forbidden` when a tool
//!   would create, approve or release a motion run ticket (D-008). [`tools::emit`] gives the
//!   bytes of `mcp-tools.json`; `fid-surface-gen` writes them and `surface-index.json`.
//! - [`text`]: the code points no tool text may hold, the same Unicode 16.0 table the bus
//!   uses for rejection reasons (`CORE.BUS.POL.PROJ`); generation refuses, never strips.
//! - [`adapter`]: a tool call becomes one bus invocation on channel `mcp` for the session's
//!   agent. The adapter never names a principal or an approval; the bus derives the
//!   principal from the channel. The bus is the only gate: when it reports that a request-only
//!   command ran, the adapter cannot undo that; it reports a violation for audit and refuses
//!   the result.
//!
//! Which tool is listed is [`tools::decide`], a pure function of the class, the declared
//! projection, `human_only` and the exposure, written to give the same answer as the bus's
//! `authorise_projection` for every command; the tests check that against the bus for every
//! command of the workspace registry and every class.

pub mod adapter;
pub mod text;
pub mod tools;

pub use adapter::{
    call_tool, AgentSession, AuditSink, BusCall, McpBus, Refusal, Severity, ToolTable, Violation, CHANNEL,
};
pub use tools::{
    decide, emit, tools, Annotations, Emitted, McpError, McpTool, ToolKind, MOTION_ALLOWLIST, MOTION_EFFECTS,
    REQUEST_ONLY_SENTENCE, SURFACE, TOOLS_FILE,
};
