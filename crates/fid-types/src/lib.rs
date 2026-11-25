//! Shared vocabulary of Metrology Maven. Every crate uses these types instead of
//! redefining them, so the wire strings stay identical everywhere.
//!
//! - [`Determinism`] and [`SideEffect`]: the closed classes of CONSTITUTION §4 and §3;
//! - [`PrincipalKind`]: the six principal kinds a bus session runs as (CORE.BUS.PRIN);
//! - [`Digest`]: `sha256:` followed by 64 lowercase hex digits;
//! - [`CommandName`] and [`CommandId`]: registry command IDs `name@major` (CONSTITUTION §3);
//! - [`text`]: the code points untrusted display text may not hold (one table for the bus and
//!   the importers).
//!
//! No dependencies, no serde: every type has `as_str` / `parse` over its wire string.
//! Errors are one small enum, [`TypesError`], with stable codes.

mod class;
mod command;
mod digest;
mod error;
pub mod text;

pub use class::{Determinism, PrincipalKind, SideEffect};
pub use command::{CommandId, CommandName};
pub use digest::Digest;
pub use error::TypesError;
