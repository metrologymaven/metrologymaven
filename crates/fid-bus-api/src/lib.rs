//! Surface-facing command bus API crate (`CORE.BUS.LINT`, CONSTITUTION C3).
//!
//! Surface crates (`fid-cli`, `fid-mcp`) depend exclusively on `fid-bus-api` for command
//! dispatch traits and canonical request/response DTOs, never directly on `fid-bus`,
//! `fid-store`, or `fid-state`.

pub mod dispatch;
pub mod envelope;
pub mod error;
pub mod router;
pub mod session;

pub use dispatch::{CommandBus, CommandHandler};
pub use envelope::{BusEnvelope, BusOutcome, CommandCall};
pub use error::{BusApiError, BusApiErrorCode, MAX_ERROR_CHARS};
pub use router::InProcessBus;
pub use session::{CallOptions, ChannelKind, PrincipalInfo, PrincipalKind, TargetHint};
