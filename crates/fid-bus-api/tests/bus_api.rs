#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus_api::{
    BusApiError, BusApiErrorCode, BusEnvelope, BusOutcome, CallOptions, ChannelKind, CommandBus, CommandCall,
    CommandHandler, InProcessBus, PrincipalInfo, PrincipalKind, TargetHint,
};
use fid_canon::{to_canonical_bytes, CValue};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::BTreeMap;

struct EchoHandler;

impl CommandHandler for EchoHandler {
    fn id(&self) -> (&str, u32) {
        ("demo.echo", 1)
    }

    fn handle(&mut self, call: &CommandCall) -> Result<CValue, BusApiError> {
        let mut out = BTreeMap::new();
        out.insert("channel".to_string(), CValue::Str(call.channel.as_str().to_string()));
        out.insert("params".to_string(), call.params.clone());
        out.insert("principal_kind".to_string(), CValue::Str(call.principal.kind.as_str().to_string()));
        Ok(CValue::Obj(out))
    }
}

struct WriteHandler;

impl CommandHandler for WriteHandler {
    fn id(&self) -> (&str, u32) {
        ("demo.write", 2)
    }

    fn handle(&mut self, _call: &CommandCall) -> Result<CValue, BusApiError> {
        Ok(CValue::Null)
    }

    fn handle_envelope(&mut self, call: &CommandCall) -> BusEnvelope {
        if call.options.dry_run {
            BusEnvelope::dry_run(call.full_command_id(), Some(call.params.clone()))
        } else {
            BusEnvelope::committed(call.full_command_id(), Digest::TOMBSTONE.as_str(), Some(call.params.clone()))
        }
    }
}

#[test]
fn in_process_bus_dispatches_and_reports_unknown_command() {
    let mut bus = InProcessBus::new();
    bus.register(Box::new(EchoHandler));
    bus.register(Box::new(WriteHandler));

    assert!(bus.supports("demo.echo", 1));
    assert!(bus.supports("demo.echo@1", 1));
    assert!(!bus.supports("demo.echo", 2));
    assert!(bus.supports("demo.write", 2));
    assert_eq!(bus.len(), 2);
    assert!(!bus.is_empty());

    let mut params = BTreeMap::new();
    params.insert("key".to_string(), CValue::Str("val".to_string()));
    let call = CommandCall::new("demo.echo", 1, CValue::Obj(params), PrincipalInfo::script("cli-test"));
    let env = bus.dispatch(&call);
    assert_eq!(env.outcome, BusOutcome::Computed);
    assert_eq!(env.command, "demo.echo@1");
    assert!(env.error_code.is_none());

    let canonical = to_canonical_bytes(&env.to_value()).unwrap();
    assert!(!canonical.is_empty());

    let missing = CommandCall::new("demo.missing", 1, CValue::Obj(BTreeMap::new()), PrincipalInfo::script("cli-test"));
    let err_env = bus.dispatch(&missing);
    assert_eq!(err_env.outcome, BusOutcome::Failed);
    assert_eq!(err_env.error_code, Some(BusApiErrorCode::NotFound));
}

#[test]
fn dry_run_and_committed_and_principal_variants_work() {
    let mut bus = InProcessBus::new();
    bus.register(Box::new(WriteHandler));

    let agent_id = Uuid::from_u128(0x0193_a5b0_1c2d_7e3f_8a4b_0000_0000_0001);
    let opts = CallOptions { dry_run: true, json: true, ..CallOptions::default() };
    let call = CommandCall::new("demo.write@2", 2, CValue::Obj(BTreeMap::new()), PrincipalInfo::agent("a", agent_id))
        .with_channel(ChannelKind::Mcp)
        .with_options(opts.clone())
        .with_target(TargetHint::Headless { project: Some("root:p/min.fid".into()), branch: None });

    let dry = bus.dispatch(&call);
    assert_eq!(dry.outcome, BusOutcome::DryRun);

    let mut live_call = call;
    live_call.options = CallOptions { dry_run: false, ..opts };
    let committed = bus.dispatch(&live_call);
    assert_eq!(committed.outcome, BusOutcome::Committed);
    assert_eq!(committed.journal_entry.as_deref(), Some(Digest::TOMBSTONE.as_str()));
}

#[test]
fn error_truncation_and_code_round_trip() {
    let huge = "é".repeat(2500);
    let err = BusApiError::policy_refused(&huge).with_bus_reason("policy", "class_forbidden");
    assert!(err.message.chars().count() <= 2000);
    assert!(err.message.ends_with("...[truncated]"));
    let env = err.to_envelope("motion.move_axis@1");
    assert_eq!(env.outcome, BusOutcome::Failed);
    assert_eq!(env.error_code, Some(BusApiErrorCode::PolicyRefused));
    assert!(env.error_details.is_some());

    for wire in [
        "usage_error",
        "validation_error",
        "unit_error",
        "policy_refused",
        "precondition_failed",
        "conflict",
        "not_found",
        "version_mismatch",
        "raw_path_refused",
        "root_escape",
        "dry_run_unsupported",
        "timeout",
        "cancelled",
        "internal_error",
        "custom_future_code",
    ] {
        assert_eq!(BusApiErrorCode::from_wire(wire).as_str(), wire);
    }
    assert_eq!(PrincipalKind::ALL.len(), 6);
}

#[test]
fn parsed_command_id_and_supports_reject_mismatched_or_malformed_major() {
    let mut bus = InProcessBus::new();
    bus.register(Box::new(EchoHandler));

    assert!(bus.supports("demo.echo", 1));
    assert!(bus.supports("demo.echo@1", 1));
    assert!(!bus.supports("demo.echo@99", 1));
    assert!(!bus.supports("demo.echo@notanumber", 1));
    assert!(!bus.supports("demo.echo@1@2", 1));

    for bad_id in ["demo.echo@99", "demo.echo@notanumber", "demo.echo@1@2"] {
        let call = CommandCall::new(bad_id, 1, CValue::Obj(BTreeMap::new()), PrincipalInfo::script("cli-test"));
        let err = call.parsed_command_id().unwrap_err();
        assert_eq!(err.code, BusApiErrorCode::ValidationError, "bad_id={bad_id}");
        let env = bus.dispatch(&call);
        assert_eq!(env.outcome, BusOutcome::Failed);
        assert_eq!(env.error_code, Some(BusApiErrorCode::ValidationError), "bad_id={bad_id}");
    }
}

#[test]
fn bus_api_error_preserves_bus_reason_with_details() {
    let mut custom = BTreeMap::new();
    custom.insert("field".to_string(), CValue::Str("source".to_string()));
    let err = BusApiError::policy_refused("forbidden")
        .with_bus_reason("policy", "class_forbidden")
        .with_details(CValue::Obj(custom));
    let env = err.to_envelope("motion.move_axis@1");
    let CValue::Obj(details) = env.error_details.unwrap() else { panic!("expected obj details") };
    assert_eq!(details.get("bus_code"), Some(&CValue::Str("policy".to_string())));
    assert_eq!(details.get("bus_reason"), Some(&CValue::Str("class_forbidden".to_string())));
    assert_eq!(details.get("field"), Some(&CValue::Str("source".to_string())));

    let err_non_obj = BusApiError::validation("bad")
        .with_bus_reason("validation", "schema")
        .with_details(CValue::Str("raw detail".to_string()));
    let env2 = err_non_obj.to_envelope("demo.echo@1");
    let CValue::Obj(details2) = env2.error_details.unwrap() else { panic!("expected obj details") };
    assert_eq!(details2.get("detail"), Some(&CValue::Str("raw detail".to_string())));
    assert_eq!(details2.get("bus_code"), Some(&CValue::Str("validation".to_string())));
    assert_eq!(details2.get("bus_reason"), Some(&CValue::Str("schema".to_string())));
}
