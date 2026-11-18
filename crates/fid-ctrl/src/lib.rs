//! Control-channel transport. Spec node `AUTO.CTRL.WIRE`.
//!
//! - [`server::Server`]: a listener bound only to `127.0.0.1` or `::1` on an ephemeral
//!   port, at most 8 connections, one thread per connection;
//! - [`discovery`]: the owner-only discovery file (port, pid, instance ID, versions);
//! - [`frame::LineReader`]: JSON-lines framing that never buffers more than the bound + 1;
//! - [`msg`]: the `{v, id, op, params}` / `{v, id, envelope | event}` schema and `hello`.
//!
//! Every input is hostile: lines are bounded (1 MiB, 16 MiB for screenshot replies),
//! each line must complete within a deadline, writes time out, and nothing panics.
//! Authentication and operations are other nodes; they plug in through [`server::Handler`].

pub mod discovery;
pub mod error;
pub mod frame;
pub mod msg;
pub mod server;

pub use discovery::Discovery;
pub use error::{StartError, WireError};
pub use frame::{Frame, LineReader};
pub use msg::{classify_stream, parse_request, OpName, Reply, ReplyBody, Request, Verdict, PROTOCOL_VERSIONS};
pub use server::{Config, Handler, Log, MemoryLog, Server, StderrLog};
