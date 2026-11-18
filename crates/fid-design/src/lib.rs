//! `fid-design`: the design-system tooling crate (spec domain `UX`). Tooling layer; not a
//! registry command. Each UX leaf adds its check as a module and a subcommand.
//!
//! - [`pkg`]: `pkg-check` and `pkg-diff` (`UX.SYSTEM.PKG`).

pub mod pkg;
