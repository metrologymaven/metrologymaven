//! Adapter exposing [`crate::Bus`] through the surface-facing [`fid_bus_api::CommandBus`] trait.

use crate::error::{BusError, Code, PolicyReason, ValidationReason};
use crate::handler::Event;
use crate::pipeline::{Bus, Call, CommandResult, Invocation};
use crate::policy::projection::RequestId;
use fid_bus_api::{BusApiErrorCode, BusEnvelope, BusOutcome, CommandBus, CommandCall};
use fid_canon::{CValue, Digest as CanonDigest};
use std::collections::BTreeMap;

/// Maps a [`BusError`] into the surface-facing [`BusApiErrorCode`].
pub fn map_bus_error_code(err: &BusError) -> BusApiErrorCode {
    match err {
        BusError::Validation(
            ValidationReason::UnitMismatch | ValidationReason::LiteralOutOfRange | ValidationReason::LiteralSyntax,
            _,
        ) => BusApiErrorCode::UnitError,
        BusError::Validation(_, _) => BusApiErrorCode::ValidationError,
        BusError::Policy(PolicyReason::RawPathRefused, _) => BusApiErrorCode::RawPathRefused,
        BusError::Policy(_, _) => BusApiErrorCode::PolicyRefused,
        BusError::Precondition(_, _) => BusApiErrorCode::PreconditionFailed,
        BusError::Conflict(_, _) => BusApiErrorCode::Conflict,
        BusError::NotFound(_, _) | BusError::HiddenCommand(_, _) => BusApiErrorCode::NotFound,
        BusError::UndeclaredEffect(_, _) => BusApiErrorCode::InternalError,
        BusError::Cancelled(_, _) => BusApiErrorCode::Cancelled,
        BusError::Unauthenticated(_, _) | BusError::UpgradeRefused(_, _) => BusApiErrorCode::PolicyRefused,
        BusError::IdempotencyMismatch(_, _) => BusApiErrorCode::Conflict,
        BusError::Internal(_, _) => BusApiErrorCode::InternalError,
    }
}

/// Maps a [`BusError`] into a failed [`BusEnvelope`] preserving the structured `{code, reason, detail}`
/// payload in `error_details`.
pub fn bus_error_to_envelope(command: &str, err: &BusError) -> BusEnvelope {
    let api_code = map_bus_error_code(err);
    let outcome = if matches!(err.code(), Code::Cancelled) { BusOutcome::Cancelled } else { BusOutcome::Failed };
    BusEnvelope {
        command: command.to_string(),
        outcome,
        result: None,
        error_code: Some(api_code),
        message: Some(format!("{}: {}", err, err.detail().as_untrusted_str())),
        error_details: Some(err.to_value()),
        request_id: None,
        journal_entry: None,
        job_id: None,
        artifacts: Vec::new(),
    }
}

/// Maps a [`CommandResult`] into a [`BusEnvelope`].
pub fn command_result_to_envelope(command: &str, res: CommandResult) -> BusEnvelope {
    match res {
        CommandResult::Done { result, entry: Some(hash), .. } => {
            BusEnvelope::committed(command, hash.to_string(), result)
        }
        CommandResult::Done { result, entry: None, .. } => {
            BusEnvelope::computed(command, result.unwrap_or(CValue::Null))
        }
        CommandResult::Pending { request_id, .. } => BusEnvelope::request_filed(command, request_id.to_string()),
        CommandResult::DryRun { events } => {
            let evs = CValue::Arr(events.iter().map(Event::to_value).collect());
            let mut m = BTreeMap::new();
            m.insert("events".to_string(), evs);
            BusEnvelope::dry_run(command, Some(CValue::Obj(m)))
        }
        CommandResult::WouldFile { command: filed_cmd } => {
            let mut m = BTreeMap::new();
            m.insert("would_file".to_string(), CValue::Str(filed_cmd.as_str().to_string()));
            BusEnvelope::dry_run(command, Some(CValue::Obj(m)))
        }
    }
}

/// Adapter implementing [`CommandBus`] over a [`Bus`] and per-call [`Call`] context.
pub struct BusApiAdapter<'a> {
    bus: &'a Bus,
    call: Call<'a>,
}

impl<'a> BusApiAdapter<'a> {
    /// Wraps `bus` and `call` for surface dispatch through [`fid_bus_api::CommandBus`].
    pub fn new(bus: &'a Bus, call: Call<'a>) -> Self {
        Self { bus, call }
    }
}

impl CommandBus for BusApiAdapter<'_> {
    fn dispatch(&mut self, req: &CommandCall) -> BusEnvelope {
        let full = req.full_command_id();
        let command = match req.parsed_command_id() {
            Ok(id) => id,
            Err(err) => return err.to_envelope(full),
        };
        let approval_of = match req.options.approval_of {
            Some(u) => match RequestId::from_uuid(u) {
                Ok(id) => Some(id),
                Err(err) => return bus_error_to_envelope(&full, &err),
            },
            None => None,
        };
        let mut inv = Invocation::new(command, req.params.clone());
        inv.dry_run = req.options.dry_run;
        inv.expected_head = req.options.expected_head.map(|d| CanonDigest(d.to_bytes()));
        inv.idempotency_key = req.options.idempotency_key.clone();
        inv.approval_of = approval_of;

        match self.bus.execute(&self.call, &inv) {
            Ok(res) => command_result_to_envelope(&full, res),
            Err(err) => bus_error_to_envelope(&full, &err),
        }
    }

    fn supports(&self, command_id: &str, major: u32) -> bool {
        let base = command_id.split('@').next().unwrap_or(command_id);
        let Ok(derived) = self.call.sessions.derive_principal(self.call.channel) else {
            return false;
        };
        self.bus.table().resolve(base, major, &derived).is_ok()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::error::Detail;

    #[test]
    fn maps_bus_errors_to_api_codes_and_preserves_details() {
        let raw = BusError::Policy(PolicyReason::RawPathRefused, Detail::new("raw path blocked"));
        let env = bus_error_to_envelope("align.set_datum@1", &raw);
        assert_eq!(env.outcome, BusOutcome::Failed);
        assert_eq!(env.error_code, Some(BusApiErrorCode::RawPathRefused));
        assert!(env.error_details.is_some());

        let unit = BusError::Validation(ValidationReason::UnitMismatch, Detail::new("mm vs rad"));
        assert_eq!(map_bus_error_code(&unit), BusApiErrorCode::UnitError);

        let nf = BusError::NotFound(crate::error::NotFoundReason::UnknownCommand, Detail::new("missing"));
        assert_eq!(map_bus_error_code(&nf), BusApiErrorCode::NotFound);

        let cancelled = BusError::Cancelled(crate::error::CancelledReason::Requested, Detail::new("stop"));
        let c_env = bus_error_to_envelope("demo.slow@1", &cancelled);
        assert_eq!(c_env.outcome, BusOutcome::Cancelled);
        assert_eq!(c_env.error_code, Some(BusApiErrorCode::Cancelled));
    }

    #[test]
    fn maps_command_results_to_envelopes() {
        let hash = CanonDigest([0xaa; 32]);
        let done = CommandResult::Done {
            result: Some(CValue::Bool(true)),
            events: Vec::new(),
            entry: Some(hash),
            activity: None,
        };
        let env = command_result_to_envelope("demo.write@1", done);
        assert_eq!(env.outcome, BusOutcome::Committed);
        assert_eq!(env.journal_entry, Some(hash.to_string()));
        assert_eq!(env.result, Some(CValue::Bool(true)));

        let req = RequestId::parse("0193a5b0-1c2d-7e3f-8a4b-000000000001").unwrap();
        let pending = CommandResult::Pending { request_id: req, entry: hash };
        let p_env = command_result_to_envelope("demo.req@1", pending);
        assert_eq!(p_env.outcome, BusOutcome::RequestFiled);
        assert_eq!(p_env.request_id, Some(req.to_string()));
    }
}
