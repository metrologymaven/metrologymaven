//! The channel description and the port trait (REQ-CORE.BUS.API-008, REQ-CORE.BUS.API-010).

use crate::call::{ApiReply, BusCall};
use crate::error::ApiError;
use fid_types::PrincipalKind;
use std::fmt;

/// The kind of channel a port sends on (REQ-CORE.BUS.API-010).
///
/// Exactly the four kinds a port can be opened as (REQ-CORE.BUS.PORT-001). There is no `gui`
/// kind: a `gui` channel derives `human` with no upgrade (REQ-CORE.BUS.PRIN-005), so no port
/// can start as `human`, and none of these four derives `human` without a confirmed upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PortKind {
    Cli,
    Python,
    Control,
    Mcp,
}

impl PortKind {
    /// Every kind, in declaration order.
    pub const ALL: &'static [PortKind] = &[PortKind::Cli, PortKind::Python, PortKind::Control, PortKind::Mcp];

    /// The wire name: `cli`, `python`, `control` or `mcp`.
    pub const fn as_str(self) -> &'static str {
        match self {
            PortKind::Cli => "cli",
            PortKind::Python => "python",
            PortKind::Control => "control",
            PortKind::Mcp => "mcp",
        }
    }
}

impl fmt::Display for PortKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A read-only description of the channel a port sends on (REQ-CORE.BUS.API-010).
///
/// Plain data that grants nothing: building or changing a `ChannelInfo` changes no channel,
/// and no operation takes one back. `session` is the session's display label
/// (`<label>#<number>`, CORE.BUS.PRIN), never the session key, which is a `fid-bus`
/// capability and never leaves it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelInfo {
    /// The port's channel kind.
    pub kind: PortKind,
    /// The channel's authenticated identity, as the bus shows it.
    pub identity: String,
    /// The session's display label, `<label>#<number>`.
    pub session: String,
}

/// One authenticated connection to a bus, as a surface holds it (REQ-CORE.BUS.API-008).
///
/// The operations take no presence provider, principal, identity, session or upgrade
/// argument: who calls, and whether they are `human`, is the bus's own derivation for the
/// port's channel. `principal_kind` reports that derivation for display; the bus enforces
/// policy on its own derivation at every call, never on this answer. The trait is object safe,
/// so surfaces hold a `Box<dyn BusPort>` or `&mut dyn BusPort`.
pub trait BusPort {
    /// The channel this port sends on.
    fn channel(&self) -> ChannelInfo;

    /// The principal kind the bus derives for this port's channel now.
    fn principal_kind(&self) -> PrincipalKind;

    /// Asks the bus to confirm a person for this session (CORE.BUS.PRIN upgrade). Who is asked,
    /// and how, is the bus's choice; the caller supplies nothing.
    fn confirm_human(&mut self) -> Result<(), ApiError>;

    /// Runs one call on this port's channel.
    fn call(&mut self, call: &BusCall) -> ApiReply;
}
