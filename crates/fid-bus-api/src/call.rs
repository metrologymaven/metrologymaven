//! One call and its reply (REQ-CORE.BUS.API-004, REQ-CORE.BUS.API-005).

use crate::error::ApiError;
use fid_canon::CValue;
use fid_types::{CommandId, Digest};

/// One call a surface asks a port to run (REQ-CORE.BUS.API-004).
///
/// It names what to run and nothing about who runs it: there is no principal, actor,
/// on-behalf-of, identity or session field. The bus takes all of those from the port's own
/// channel, so a caller cannot change them through a call.
#[derive(Debug, Clone, PartialEq)]
pub struct BusCall {
    /// The registry command ID, `name@major`.
    pub command: CommandId,
    /// The parameter document, validated by the bus against the command's schema.
    pub params: CValue,
    /// Run the command as a dry run: compute what it would do and commit nothing.
    pub dry_run: bool,
}

/// The reply to one [`BusCall`] (REQ-CORE.BUS.API-005).
#[derive(Debug, Clone, PartialEq)]
pub enum ApiReply {
    /// The command ran and its journal entry was committed.
    Committed {
        /// The `sha256:` digest of the committed entry.
        entry: Digest,
        /// The command's result, if it has one.
        result: Option<CValue>,
    },
    /// The command ran and committed nothing (R0, R1).
    Computed {
        /// The command's result, if it has one.
        result: Option<CValue>,
    },
    /// The command filed a request for someone else to decide.
    RequestFiled {
        /// The ID of the filed request.
        request_id: String,
    },
    /// A dry run ran; its result is required.
    DryRun {
        /// What the command would have done.
        result: CValue,
    },
    /// The call failed.
    Failed(ApiError),
}
