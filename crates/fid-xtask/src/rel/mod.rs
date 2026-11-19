//! Release tasks (spec nodes `ECO.REL.PROC` and `ECO.REL.SIGN`).

pub mod keys;
pub mod notes;
pub mod runbook;
pub mod sbom;
pub mod sums;
pub mod version;

/// Every `cargo xtask rel <subcommand>` the binary dispatches. `runbook-check` requires the
/// `tool` of every `available` runbook step to name one of these; a test checks that each
/// appears in the binary's usage text.
pub const SUBCOMMANDS: [&str; 8] =
    ["version-check", "version-compare", "sums", "runbook-check", "runbook-render", "sbom", "keys-check", "notes"];
