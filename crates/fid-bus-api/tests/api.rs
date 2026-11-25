//! Acceptance criteria AC-2 .. AC-5 of spec node CORE.BUS.API. Expected values are
//! hand-labelled from the leaf's requirements and from `fixtures/core/bus/api/errors.json`
//! (made by its own Python script, not by this crate). Runs on wasm32 too: the fixture is
//! compiled in.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus_api::{
    ApiError, ApiReply, ApiStage, BusCall, BusPort, ChannelInfo, InvalidApiError, PortKind, PrincipalKind,
    INVALID_API_ERROR,
};
use fid_canon::CValue;
use fid_types::{CommandId, Digest};
use std::collections::BTreeMap;

const ERRORS: &str = include_str!("../../../fixtures/core/bus/api/errors.json");

fn obj(pairs: &[(&str, CValue)]) -> CValue {
    CValue::Obj(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

fn digest() -> Digest {
    Digest::parse("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").unwrap()
}

// ------------------------------------------------------------------ AC-2

/// Every field of `BusCall` and of each `ApiReply` variant, matched with no `..`: a field added
/// to either type fails to compile here until this test and REQ-004 or REQ-005 change.
fn describe(call: &BusCall, reply: &ApiReply) -> String {
    let BusCall { command, params, dry_run } = call;
    let head = format!("{command} dry_run={dry_run} params={}", params != &CValue::Null);
    let tail = match reply {
        ApiReply::Committed { entry, result } => format!("committed {entry} {}", result.is_some()),
        ApiReply::Computed { result } => format!("computed {}", result.is_some()),
        ApiReply::RequestFiled { request_id } => format!("request_filed {request_id}"),
        ApiReply::DryRun { result } => format!("dry_run {}", result != &CValue::Null),
        ApiReply::Failed(error) => format!("failed {}/{}", error.code(), error.reason()),
    };
    format!("{head} -> {tail}")
}

#[test]
fn ac2_call_and_reply_have_exactly_the_specified_fields() {
    let call = BusCall {
        command: CommandId::parse("demo.show_thing@1").unwrap(),
        params: obj(&[("n", CValue::I64(1))]),
        dry_run: false,
    };
    let failed = ApiError::new("not_found", "unknown_command", "", ApiStage::Resolve).unwrap();
    let replies = [
        ApiReply::Committed { entry: digest(), result: None },
        ApiReply::Computed { result: Some(obj(&[])) },
        ApiReply::RequestFiled { request_id: "req-1".to_string() },
        ApiReply::DryRun { result: obj(&[("would", CValue::Bool(true))]) },
        ApiReply::Failed(failed),
    ];
    let got: Vec<String> = replies.iter().map(|r| describe(&call, r)).collect();
    let digest_str = digest().to_string();
    assert_eq!(
        got,
        [
            format!("demo.show_thing@1 dry_run=false params=true -> committed {digest_str} false"),
            "demo.show_thing@1 dry_run=false params=true -> computed true".to_string(),
            "demo.show_thing@1 dry_run=false params=true -> request_filed req-1".to_string(),
            "demo.show_thing@1 dry_run=false params=true -> dry_run true".to_string(),
            "demo.show_thing@1 dry_run=false params=true -> failed not_found/unknown_command".to_string(),
        ]
    );
}

#[test]
fn ac2_channel_info_and_port_kind_are_closed() {
    // REQ-010: exactly `kind`, `identity` and `session`, all plain data.
    let info = ChannelInfo { kind: PortKind::Cli, identity: "user-a".to_string(), session: "fid/42#1".to_string() };
    let ChannelInfo { kind, identity, session } = info.clone();
    let mut copy = info.clone();
    copy.session.push('x');
    assert_eq!((kind, identity.as_str(), session.as_str()), (PortKind::Cli, "user-a", "fid/42#1"));
    assert_ne!(copy, info);

    // REQ-010: exactly four port kinds, and no `gui`. An exhaustive match with four arms.
    let wire = |k: PortKind| match k {
        PortKind::Cli => "cli",
        PortKind::Python => "python",
        PortKind::Control => "control",
        PortKind::Mcp => "mcp",
    };
    let names: Vec<&str> = PortKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(names, ["cli", "python", "control", "mcp"]);
    for k in PortKind::ALL {
        assert_eq!(k.as_str(), wire(*k));
        assert_eq!(k.to_string(), wire(*k));
    }
}

// ------------------------------------------------------------------ AC-3

fn field<'a>(case: &'a BTreeMap<String, CValue>, key: &str) -> &'a str {
    match case.get(key) {
        Some(CValue::Str(s)) => s,
        other => panic!("case field {key}: {other:?}"),
    }
}

#[test]
fn ac3_error_fixture() {
    let doc = fid_canon::parse(ERRORS.as_bytes()).unwrap();
    let CValue::Obj(doc) = doc else { panic!("fixture is not an object") };
    let Some(CValue::Arr(cases)) = doc.get("cases") else { panic!("no cases") };
    let (mut ok, mut bad) = (0, 0);
    for case in cases {
        let CValue::Obj(case) = case else { panic!("case is not an object") };
        let name = field(case, "name");
        let (code, reason, detail) = (field(case, "code"), field(case, "reason"), field(case, "detail"));
        let stage = ApiStage::parse(field(case, "stage")).unwrap_or_else(|| panic!("{name}: stage"));
        let built = ApiError::new(code, reason, detail, stage);
        match field(case, "expect") {
            "ok" => {
                let e = built.unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!((e.code(), e.reason(), e.detail(), e.stage()), (code, reason, detail, stage), "{name}");
                ok += 1;
            }
            "invalid_api_error" => {
                let e: InvalidApiError = built.err().unwrap_or_else(|| panic!("{name}: built"));
                assert_eq!(e.code(), INVALID_API_ERROR, "{name}");
                bad += 1;
            }
            other => panic!("{name}: expect {other}"),
        }
    }
    // The fixture's shape (counted by hand from make_cases.py): 21 valid and 12 invalid cases.
    assert_eq!((ok, bad), (21, 12));
}

#[test]
fn ac3_every_stage_is_kept() {
    for stage in ApiStage::ALL {
        let e = ApiError::new("internal", "storage", "x", *stage).unwrap();
        assert_eq!(e.stage(), *stage);
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_principal_kind_wire_strings() {
    let all = [
        (PrincipalKind::Human, "human"),
        (PrincipalKind::Agent, "agent"),
        (PrincipalKind::Script, "script"),
        (PrincipalKind::Service, "service"),
        (PrincipalKind::Device, "device"),
        (PrincipalKind::System, "system"),
    ];
    assert_eq!(PrincipalKind::ALL.len(), all.len());
    for (kind, wire) in all {
        assert_eq!(kind.as_str(), wire);
        assert_eq!(PrincipalKind::parse(kind.as_str()), Ok(kind));
    }
    for bad in ["Human", "HUMAN", "user", ""] {
        let e = PrincipalKind::parse(bad).unwrap_err();
        assert_eq!((e.code(), e.input()), ("bad_principal_kind", bad));
    }
}

// ------------------------------------------------------------------ AC-5

/// A test double: it records calls and pretends a confirmation makes it `human`. It shows the
/// trait can be implemented and used as `&mut dyn BusPort`; it reaches no bus.
struct Double {
    calls: Vec<BusCall>,
    confirmed: bool,
}

impl BusPort for Double {
    fn channel(&self) -> ChannelInfo {
        ChannelInfo { kind: PortKind::Python, identity: "user-a".to_string(), session: "py#7".to_string() }
    }

    fn principal_kind(&self) -> PrincipalKind {
        if self.confirmed {
            PrincipalKind::Human
        } else {
            PrincipalKind::Script
        }
    }

    fn confirm_human(&mut self) -> Result<(), ApiError> {
        self.confirmed = true;
        Ok(())
    }

    fn call(&mut self, call: &BusCall) -> ApiReply {
        self.calls.push(call.clone());
        ApiReply::Computed { result: Some(CValue::U64(self.calls.len() as u64)) }
    }
}

#[test]
fn ac5_the_trait_is_object_safe_and_takes_no_identity() {
    // The exact signatures (REQ-008), as the trait fixes them for every implementor: no presence
    // provider, principal, identity, session or upgrade argument; `confirm_human` takes only
    // `&mut self`.
    let _channel: fn(&Double) -> ChannelInfo = <Double as BusPort>::channel;
    let _principal: fn(&Double) -> PrincipalKind = <Double as BusPort>::principal_kind;
    let _confirm: fn(&mut Double) -> Result<(), ApiError> = <Double as BusPort>::confirm_human;
    let _call: fn(&mut Double, &BusCall) -> ApiReply = <Double as BusPort>::call;

    let mut double = Double { calls: Vec::new(), confirmed: false };
    let port: &mut dyn BusPort = &mut double;
    let c = BusCall { command: CommandId::parse("demo.show_thing@1").unwrap(), params: obj(&[]), dry_run: true };

    assert_eq!(port.channel().session, "py#7");
    assert_eq!(port.principal_kind(), PrincipalKind::Script);
    assert_eq!(port.call(&c), ApiReply::Computed { result: Some(CValue::U64(1)) });
    assert_eq!(port.confirm_human(), Ok(()));
    assert_eq!(port.principal_kind(), PrincipalKind::Human);

    let mut boxed: Box<dyn BusPort + Send> = Box::new(Double { calls: Vec::new(), confirmed: false });
    assert_eq!(boxed.call(&c), ApiReply::Computed { result: Some(CValue::U64(1)) });
    assert_eq!(boxed.channel().kind, PortKind::Python);
    assert_eq!(double.calls, [c]);
}
