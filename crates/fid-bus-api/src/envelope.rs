//! Command invocation call and canonical result envelope types.

use crate::error::{BusApiError, BusApiErrorCode};
use crate::session::{CallOptions, ChannelKind, PrincipalInfo, TargetHint};
use fid_canon::CValue;
use fid_types::CommandId;
use std::collections::BTreeMap;
use std::fmt;

/// Normalized outcome of a bus invocation (mirrors `AUTO.SURF.ENV` `Outcome`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BusOutcome {
    Committed,
    Computed,
    DryRun,
    RequestFiled,
    Accepted,
    Failed,
    Cancelled,
    NotRun,
    NoReply,
    Unknown(String),
}

impl BusOutcome {
    pub fn as_str(&self) -> &str {
        match self {
            BusOutcome::Committed => "committed",
            BusOutcome::Computed => "computed",
            BusOutcome::DryRun => "dry_run",
            BusOutcome::RequestFiled => "request_filed",
            BusOutcome::Accepted => "accepted",
            BusOutcome::Failed => "failed",
            BusOutcome::Cancelled => "cancelled",
            BusOutcome::NotRun => "not_run",
            BusOutcome::NoReply => "no_reply",
            BusOutcome::Unknown(s) => s.as_str(),
        }
    }

    pub fn from_wire(s: &str) -> Self {
        match s {
            "committed" => BusOutcome::Committed,
            "computed" => BusOutcome::Computed,
            "dry_run" => BusOutcome::DryRun,
            "request_filed" => BusOutcome::RequestFiled,
            "accepted" => BusOutcome::Accepted,
            "failed" => BusOutcome::Failed,
            "cancelled" => BusOutcome::Cancelled,
            "not_run" => BusOutcome::NotRun,
            "no_reply" => BusOutcome::NoReply,
            other => BusOutcome::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for BusOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Request sent from a surface (`fid-cli`, `fid-mcp`) across [`crate::CommandBus`].
#[derive(Debug, Clone, PartialEq)]
pub struct CommandCall {
    /// Command base name (`domain.verb`) or full `name@major`.
    pub command_id: String,
    pub major: u32,
    pub params: CValue,
    pub channel: ChannelKind,
    pub principal: PrincipalInfo,
    pub options: CallOptions,
    pub target: TargetHint,
}

impl CommandCall {
    /// Builds a CLI-channel call with default options and target hint.
    pub fn new(command_id: impl Into<String>, major: u32, params: CValue, principal: PrincipalInfo) -> Self {
        Self {
            command_id: command_id.into(),
            major,
            params,
            channel: ChannelKind::Cli,
            principal,
            options: CallOptions::default(),
            target: TargetHint::Auto,
        }
    }

    pub fn with_channel(mut self, channel: ChannelKind) -> Self {
        self.channel = channel;
        self
    }

    pub fn with_options(mut self, options: CallOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_target(mut self, target: TargetHint) -> Self {
        self.target = target;
        self
    }

    /// Returns the bare command name without any `@major` suffix.
    pub fn base_name(&self) -> &str {
        self.command_id.split('@').next().unwrap_or(self.command_id.as_str())
    }

    /// Returns the canonical `name@major` command ID string.
    pub fn full_command_id(&self) -> String {
        format!("{}@{}", self.base_name(), self.major)
    }

    /// Parses and validates the full `CommandId`.
    pub fn parsed_command_id(&self) -> Result<CommandId, BusApiError> {
        let full = self.full_command_id();
        CommandId::parse(&full).map_err(|e| BusApiError::validation(format!("invalid command ID `{full}`: {e}")))
    }
}

/// Canonical result returned by [`crate::CommandBus::dispatch`].
#[derive(Debug, Clone, PartialEq)]
pub struct BusEnvelope {
    pub command: String,
    pub outcome: BusOutcome,
    pub result: Option<CValue>,
    pub error_code: Option<BusApiErrorCode>,
    pub message: Option<String>,
    pub error_details: Option<CValue>,
    pub request_id: Option<String>,
    pub journal_entry: Option<String>,
    pub job_id: Option<String>,
    pub artifacts: Vec<String>,
}

impl BusEnvelope {
    pub fn bare(command: impl Into<String>, outcome: BusOutcome) -> Self {
        Self {
            command: command.into(),
            outcome,
            result: None,
            error_code: None,
            message: None,
            error_details: None,
            request_id: None,
            journal_entry: None,
            job_id: None,
            artifacts: Vec::new(),
        }
    }

    pub fn computed(command: impl Into<String>, result: CValue) -> Self {
        Self { result: Some(result), ..Self::bare(command, BusOutcome::Computed) }
    }

    pub fn committed(command: impl Into<String>, journal_entry: impl Into<String>, result: Option<CValue>) -> Self {
        Self { journal_entry: Some(journal_entry.into()), result, ..Self::bare(command, BusOutcome::Committed) }
    }

    pub fn dry_run(command: impl Into<String>, result: Option<CValue>) -> Self {
        Self { result, ..Self::bare(command, BusOutcome::DryRun) }
    }

    pub fn request_filed(command: impl Into<String>, request_id: impl Into<String>) -> Self {
        Self { request_id: Some(request_id.into()), ..Self::bare(command, BusOutcome::RequestFiled) }
    }

    pub fn accepted(command: impl Into<String>, job_id: impl Into<String>) -> Self {
        Self { job_id: Some(job_id.into()), ..Self::bare(command, BusOutcome::Accepted) }
    }

    pub fn failed(
        command: impl Into<String>,
        code: BusApiErrorCode,
        message: impl Into<String>,
        details: Option<CValue>,
    ) -> Self {
        Self {
            error_code: Some(code),
            message: Some(message.into()),
            error_details: details,
            ..Self::bare(command, BusOutcome::Failed)
        }
    }

    pub fn cancelled(command: impl Into<String>, job_id: Option<String>) -> Self {
        Self { job_id, ..Self::bare(command, BusOutcome::Cancelled) }
    }

    pub fn not_run(command: impl Into<String>) -> Self {
        Self::bare(command, BusOutcome::NotRun)
    }

    pub fn no_reply(command: impl Into<String>, job_id: Option<String>) -> Self {
        Self { job_id, ..Self::bare(command, BusOutcome::NoReply) }
    }

    pub fn from_error(command: impl Into<String>, err: &BusApiError) -> Self {
        err.to_envelope(command)
    }

    /// Serializes this envelope into the `envelope-v1` canonical JSON value shape.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("envelope_version".to_string(), CValue::Num(1.0));
        o.insert("command_id".to_string(), CValue::Str(self.command.clone()));
        o.insert("outcome".to_string(), CValue::Str(self.outcome.as_str().to_string()));
        if self.outcome == BusOutcome::Failed {
            let mut err = BTreeMap::new();
            let code = self.error_code.as_ref().map_or("internal_error", BusApiErrorCode::as_str);
            err.insert("code".to_string(), CValue::Str(code.to_string()));
            err.insert("message".to_string(), CValue::Str(self.message.clone().unwrap_or_default()));
            if let Some(d) = &self.error_details {
                err.insert("details".to_string(), d.clone());
            }
            o.insert("error".to_string(), CValue::Obj(err));
        }
        if let Some(h) = &self.journal_entry {
            o.insert("journal_entry_hash".to_string(), CValue::Str(h.clone()));
        }
        if let Some(j) = &self.job_id {
            o.insert("job_id".to_string(), CValue::Str(j.clone()));
        }
        if let Some(r) = &self.request_id {
            o.insert("request_id".to_string(), CValue::Str(r.clone()));
        }
        if let Some(res) = &self.result {
            o.insert("result".to_string(), res.clone());
        }
        CValue::Obj(o)
    }
}
