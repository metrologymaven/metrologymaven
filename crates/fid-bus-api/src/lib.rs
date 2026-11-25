//! The bus API a surface sees. Spec node `CORE.BUS.API`.
//!
//! This is the one CORE crate a `[surfaces]` crate may link (`bus_api` in
//! `lint-allowlist.toml`, REQ-CORE.BUS.LINT-004). It is deliberately thin: plain data and one
//! trait, over `fid-canon` and `fid-types` only.
//!
//! - [`BusCall`]: one call in CORE form (command ID, parameter document, dry-run flag) and
//!   nothing else: no principal, actor, on-behalf-of, identity or session field;
//! - [`ApiReply`] and [`ApiError`]: the success forms, and a bus error's code, reason, detail
//!   and the [`ApiStage`] that raised it;
//! - [`PortKind`] and [`ChannelInfo`]: a read-only description of the channel a port sends on;
//! - [`BusPort`]: the trait an in-process bus session implements (in `fid-bus`,
//!   `CORE.BUS.PORT`);
//! - [`PrincipalKind`], re-exported from `fid-types`.
//!
//! The crate holds no bus, no session table and no presence provider. Nothing here runs a
//! command, opens a session or makes anyone `human`: a value of any type in this crate grants
//! nothing. A surface that implements [`BusPort`] itself only fools itself, because this crate
//! leads to no bus. The bus derives the principal of every call on its own and enforces policy
//! on that derivation, never on what a port reports.

mod call;
mod error;
mod port;

pub use call::{ApiReply, BusCall};
pub use error::{ApiError, ApiStage, InvalidApiError, INVALID_API_ERROR, MAX_CODE_BYTES, MAX_DETAIL_CHARS};
pub use fid_types::PrincipalKind;
pub use port::{BusPort, ChannelInfo, PortKind};
