//! Electronic signature catalogue and ceremony for Metrology Maven (spec domain
//! `TRUST.ESIG`).
//!
//! - [`meaning`] (`TRUST.ESIG.MEANING`): the signature meanings catalogue
//!   (built-in codes `AUTHORED`, `REVIEWED`, `APPROVED`, `RELEASED` plus
//!   validated `X_`-prefixed tenant meanings from the active governance profile)
//!   and the `sign.list_meanings@1` R0 read command.
//!
//! Nothing here panics on any input: every outcome is a returned `Result`.

pub mod meaning;

use fid_registry::{CommandDef, DefError};

/// Every command definition declared by `fid-esig`, for `fid-registry-build`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(meaning::declarations().into_iter().map(|d| d.def).collect())
}
