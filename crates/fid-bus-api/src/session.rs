//! Session identity, target selection hints, and invocation options shared across
//! `fid-cli`, `fid-mcp`, and `fid-bus`.

use fid_types::Digest;
use fid_uuid::Uuid;
use std::fmt;

/// Surface channel kind reported by callers to the bus boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    Cli,
    Mcp,
    Control,
    Python,
    Service,
    Internal,
}

impl ChannelKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            ChannelKind::Cli => "cli",
            ChannelKind::Mcp => "mcp",
            ChannelKind::Control => "control",
            ChannelKind::Python => "python",
            ChannelKind::Service => "service",
            ChannelKind::Internal => "internal",
        }
    }
}

impl fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Principal kind aligned with `CORE.BUS.PRIN` and `AUTO.SURF.ROOTS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrincipalKind {
    Human,
    Agent,
    Script,
    Service,
    Device,
    System,
}

impl PrincipalKind {
    pub const ALL: &'static [PrincipalKind] = &[
        PrincipalKind::Human,
        PrincipalKind::Agent,
        PrincipalKind::Script,
        PrincipalKind::Service,
        PrincipalKind::Device,
        PrincipalKind::System,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            PrincipalKind::Human => "human",
            PrincipalKind::Agent => "agent",
            PrincipalKind::Script => "script",
            PrincipalKind::Service => "service",
            PrincipalKind::Device => "device",
            PrincipalKind::System => "system",
        }
    }
}

impl fmt::Display for PrincipalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Caller session principal descriptor passed across the surface/bus boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrincipalInfo {
    pub kind: PrincipalKind,
    pub id: String,
    pub session_id: Option<Uuid>,
}

impl PrincipalInfo {
    pub fn new(kind: PrincipalKind, id: impl Into<String>, session_id: Option<Uuid>) -> Self {
        Self { kind, id: id.into(), session_id }
    }

    pub fn script(id: impl Into<String>) -> Self {
        Self::new(PrincipalKind::Script, id, None)
    }

    pub fn human(id: impl Into<String>) -> Self {
        Self::new(PrincipalKind::Human, id, None)
    }

    pub fn agent(id: impl Into<String>, session_id: Uuid) -> Self {
        Self::new(PrincipalKind::Agent, id, Some(session_id))
    }
}

/// Target routing hint selected by the surface before dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TargetHint {
    #[default]
    Auto,
    App {
        socket_or_endpoint: Option<String>,
    },
    Headless {
        project: Option<String>,
        branch: Option<String>,
    },
    None,
}

/// Per-call execution options forwarded across the bus boundary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallOptions {
    pub dry_run: bool,
    pub timeout_ms: Option<u64>,
    pub reason: Option<String>,
    pub expected_head: Option<Digest>,
    pub idempotency_key: Option<String>,
    pub approval_of: Option<Uuid>,
    pub human_confirmed: bool,
    pub json: bool,
}
