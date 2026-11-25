//! Acceptance criteria AC-1 .. AC-5 of spec node AUTO.SURF.INPROC, plus checks of the bus-code
//! table against the bus error catalogue (`schema/bus-errors.json`) and against the root gate
//! (`fid_surface::roots`). Expected envelopes are hand-labelled in
//! `fixtures/auto/surf/inproc/replies.json` from the leaf's table; the fake port and the
//! counting port are test code here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus_api::{ApiError, ApiReply, ApiStage, BusCall, BusPort, ChannelInfo, PortKind, PrincipalKind};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_surface::inproc::{
    bus_origin, envelope_for, error_code_for, hidden_as_unknown_major, InProcTransport, BUS_ERROR_TABLE, CANCELLED,
    MAX_QUEUED_EVENTS, NO_LIVE_JOB,
};
use fid_surface::jobs::{
    BusEvent, BusRequest, BusTransport, CancelStatus, CountingKeySource, InvocationId, ManualClock, Purpose, Reason,
    SendError, State, Tracker,
};
use fid_surface::roots::{PrincipalKind as RootKind, RootError};
use fid_surface::{Envelope, ErrorCode, Outcome};
use fid_types::{CommandId, Digest};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CMD: &str = "demo.show_thing@1";
const MS200: Duration = Duration::from_millis(200);
/// Timestamp of the tracker's idempotency keys.
const KEY_MS: u64 = 0x0190_1234_5678;
/// Timestamp of the transport's call IDs.
const CALL_MS: u64 = 0x0190_8765_4321;

type Obj = BTreeMap<String, CValue>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(rel: &str) -> CValue {
    let bytes = std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    parse(&bytes).unwrap()
}

fn json(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: {other:?}"),
    }
}

fn cmd() -> CommandId {
    CommandId::parse(CMD).unwrap()
}

fn api_error(code: &str, reason: &str, detail: &str, stage: ApiStage) -> ApiError {
    ApiError::new(code, reason, detail, stage).unwrap()
}

fn canonical(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn encoded(e: &Envelope) -> String {
    String::from_utf8(e.encode().unwrap()).unwrap()
}

// ------------------------------------------------------------------ the fake port

/// What the fake port saw, shared with the test after the port is boxed into the transport.
#[derive(Debug, Default)]
struct Log {
    calls: Vec<BusCall>,
    confirms: u32,
    kind_reads: u32,
}

/// A scripted [`BusPort`]: answers calls from `replies` (then `Computed {"n": 1}`), starts as
/// `script` and becomes `human` only after a `confirm_human` that `confirm_ok` lets succeed.
struct FakePort {
    log: Arc<Mutex<Log>>,
    replies: VecDeque<ApiReply>,
    kind: PrincipalKind,
    confirm_ok: bool,
}

impl FakePort {
    fn new(replies: Vec<ApiReply>) -> (Arc<Mutex<Log>>, FakePort) {
        let log = Arc::new(Mutex::new(Log::default()));
        let port =
            FakePort { log: Arc::clone(&log), replies: replies.into(), kind: PrincipalKind::Script, confirm_ok: true };
        (log, port)
    }
}

fn n1() -> ApiReply {
    ApiReply::Computed { result: Some(json(r#"{"n":1}"#)) }
}

impl BusPort for FakePort {
    fn channel(&self) -> ChannelInfo {
        ChannelInfo { kind: PortKind::Cli, identity: "user-a".to_string(), session: "cli#1".to_string() }
    }

    fn principal_kind(&self) -> PrincipalKind {
        self.log.lock().unwrap().kind_reads += 1;
        self.kind
    }

    fn confirm_human(&mut self) -> Result<(), ApiError> {
        self.log.lock().unwrap().confirms += 1;
        if self.confirm_ok {
            self.kind = PrincipalKind::Human;
            Ok(())
        } else {
            Err(api_error("upgrade_refused", "declined", "no_presence_provider", ApiStage::Session))
        }
    }

    fn call(&mut self, call: &BusCall) -> ApiReply {
        self.log.lock().unwrap().calls.push(call.clone());
        self.replies.pop_front().unwrap_or_else(n1)
    }
}

// ------------------------------------------------------------------ a recording wrapper

/// Wraps the transport under test, records every event the tracker reads and, while `hold` is
/// set, keeps `Finished` events back so the tracker stays in `Accepted`.
struct Recording {
    inner: InProcTransport,
    seen: Vec<BusEvent>,
    hold: bool,
    held: VecDeque<BusEvent>,
}

impl BusTransport for Recording {
    fn send(&mut self, request: &BusRequest) -> Result<(), SendError> {
        self.inner.send(request)
    }

    fn recv(&mut self) -> Option<BusEvent> {
        if !self.hold {
            if let Some(e) = self.held.pop_front() {
                return Some(e);
            }
        }
        loop {
            let e = self.inner.recv()?;
            self.seen.push(e.clone());
            if self.hold && matches!(e, BusEvent::Finished { .. }) {
                self.held.push_back(e);
                continue;
            }
            return Some(e);
        }
    }
}

type T = Tracker<Recording, ManualClock, CountingKeySource>;

fn transport(port: FakePort) -> InProcTransport {
    InProcTransport::new(Box::new(port), Box::new(CountingKeySource::new(CALL_MS)))
}

fn tracker(port: FakePort) -> T {
    let rec = Recording { inner: transport(port), seen: Vec::new(), hold: false, held: VecDeque::new() };
    Tracker::new(rec, ManualClock::new(), CountingKeySource::new(KEY_MS))
}

fn run(t: &mut T, params: CValue) -> InvocationId {
    let id = t.submit(cmd(), params, MS200).unwrap();
    t.pump(&mut ());
    id
}

fn is_uuid_v7(u: &Uuid) -> bool {
    let s = u.to_string();
    s.len() == 36 && s.as_bytes()[14] == b'7' && matches!(s.as_bytes()[19], b'8' | b'9' | b'a' | b'b')
}

// ------------------------------------------------------------------ AC-1

/// AC-1: one call with the request's command and parameters and `dry_run` false; `Accepted`
/// then `Finished` under one fresh UUIDv7 call ID; a `computed` envelope with no `job_id`.
#[test]
fn ac1_one_call_accepted_then_finished() {
    let (log, port) = FakePort::new(vec![n1()]);
    let mut t = tracker(port);
    let id = run(&mut t, json("{}"));

    let calls = log.lock().unwrap().calls.clone();
    assert_eq!(calls, vec![BusCall { command: cmd(), params: json("{}"), dry_run: false }]);

    let key = t.key(id).unwrap();
    let seen = t.transport().seen.clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    let BusEvent::Accepted { key: k1, job: j1 } = &seen[0] else { panic!("first event: {:?}", seen[0]) };
    let BusEvent::Finished { key: k2, job: j2, envelope } = &seen[1] else { panic!("second event: {:?}", seen[1]) };
    assert_eq!((*k1, *k2), (key, key));
    assert_eq!(j1, j2, "one call ID");
    assert_ne!(*j1, key, "the call ID is fresh, not the idempotency key");
    assert!(is_uuid_v7(j1), "{j1}");

    let report = t.report(id).unwrap();
    assert_eq!(report.reason, Reason::Final);
    assert_eq!(&report.envelope, envelope.as_ref());
    let env = &report.envelope;
    assert_eq!(env.outcome, Outcome::Computed);
    assert_eq!(env.result, Some(json(r#"{"n":1}"#)));
    assert_eq!(env.job_id, None);
    env.validate().unwrap();
    assert_eq!(
        encoded(env),
        r#"{"command_id":"demo.show_thing@1","envelope_version":1,"outcome":"computed","result":{"n":1}}"#
    );
}

/// REQ-002: every call gets its own call ID, and the envelope never names one.
#[test]
fn call_ids_are_fresh_per_call() {
    let (log, port) = FakePort::new(vec![]);
    let mut t = tracker(port);
    let a = run(&mut t, json("{}"));
    let b = run(&mut t, json(r#"{"x":2}"#));
    assert_eq!(log.lock().unwrap().calls.len(), 2);
    let jobs: Vec<Uuid> = t
        .transport()
        .seen
        .iter()
        .filter_map(|e| match e {
            BusEvent::Accepted { job, .. } => Some(*job),
            _ => None,
        })
        .collect();
    assert_eq!(jobs.len(), 2);
    assert_ne!(jobs[0], jobs[1]);
    for id in [a, b] {
        assert_eq!(t.report(id).unwrap().envelope.job_id, None);
    }
}

// ------------------------------------------------------------------ AC-2

fn reply_of(v: &CValue) -> ApiReply {
    let o = obj(v);
    let result = o.get("result").cloned();
    match st(o, "kind") {
        "committed" => ApiReply::Committed { entry: Digest::parse(st(o, "entry")).unwrap(), result },
        "computed" => ApiReply::Computed { result },
        "request_filed" => ApiReply::RequestFiled { request_id: st(o, "request_id").to_string() },
        "dry_run" => ApiReply::DryRun { result: result.unwrap() },
        "failed" => ApiReply::Failed(api_error(
            st(o, "code"),
            st(o, "reason"),
            st(o, "detail"),
            ApiStage::parse(st(o, "stage")).unwrap(),
        )),
        other => panic!("unknown reply kind {other}"),
    }
}

fn cases() -> Vec<(String, ApiReply, CValue)> {
    let doc = read_json("fixtures/auto/surf/inproc/replies.json");
    let doc = obj(&doc);
    assert_eq!(st(doc, "command"), CMD);
    let Some(CValue::Arr(cases)) = doc.get("cases") else { panic!("no cases") };
    cases
        .iter()
        .map(|c| {
            let c = obj(c);
            (st(c, "name").to_string(), reply_of(&c["reply"]), c["expected"].clone())
        })
        .collect()
}

/// AC-2: each scripted reply, tracked to the end, gives the hand-labelled envelope byte for
/// byte; `envelope_for` alone gives the same bytes.
#[test]
fn ac2_fixture_replies_give_the_labelled_envelopes() {
    let cases = cases();
    for (name, reply, expected) in &cases {
        let (log, port) = FakePort::new(vec![reply.clone()]);
        let mut t = tracker(port);
        let id = run(&mut t, json("{}"));
        assert_eq!(log.lock().unwrap().calls.len(), 1, "{name}");
        let report = t.report(id).unwrap_or_else(|| panic!("{name}: no report"));
        assert_eq!(report.reason, Reason::Final, "{name}");
        assert_eq!(encoded(&report.envelope), canonical(expected), "{name}");
        assert_eq!(encoded(&envelope_for(&cmd(), reply)), canonical(expected), "{name}");
    }

    // Coverage: every success form, every table row, `cancelled` and an unknown code.
    let names: Vec<&str> = cases.iter().map(|(n, _, _)| n.as_str()).collect();
    for kind in ["committed", "computed", "request_filed", "dry_run"] {
        assert!(
            cases.iter().any(|(_, r, _)| match (kind, r) {
                ("committed", ApiReply::Committed { .. }) => true,
                ("computed", ApiReply::Computed { .. }) => true,
                ("request_filed", ApiReply::RequestFiled { request_id }) => !request_id.is_empty(),
                ("dry_run", ApiReply::DryRun { .. }) => true,
                _ => false,
            }),
            "no case for {kind}"
        );
    }
    let failures: Vec<&ApiError> = cases
        .iter()
        .filter_map(|(_, r, _)| match r {
            ApiReply::Failed(e) => Some(e),
            _ => None,
        })
        .collect();
    for (row, (code, reason, _)) in BUS_ERROR_TABLE.iter().enumerate() {
        let hit = failures.iter().any(|e| {
            BUS_ERROR_TABLE.iter().position(|(c, r, _)| *c == e.code() && (r.is_none() || *r == Some(e.reason())))
                == Some(row)
        });
        assert!(hit, "no case for table row {code}/{reason:?}");
    }
    assert!(failures.iter().any(|e| e.code() == CANCELLED), "no cancelled case");
    assert!(failures.iter().any(|e| e.code() == "future_code"), "no unknown-code case");
    assert!(names.contains(&"unknown_code"));
}

/// REQ-003 and D-045: a dry run never looks committed or computed.
#[test]
fn dry_run_never_looks_committed() {
    let env = envelope_for(&cmd(), &ApiReply::DryRun { result: json(r#"{"events":[]}"#) });
    assert_eq!(env.outcome, Outcome::DryRun);
    assert_eq!(env.journal_entry_hash, None);
    assert_eq!(env.job_id, None);
    env.validate().unwrap();
}

/// D-035 (maintainer decision 2026-11-25): a `hidden_command` failure from any stage, whatever
/// its reason and detail, gives exactly the bytes of the bus's `not_found/unknown_major`
/// refusal of the same ID from stage `resolve`. The refusal here carries the detail
/// `fid_bus::validate::CommandTable::resolve` writes; `tests/inproc_bus.rs` takes both sides
/// from a real bus.
#[test]
fn hidden_command_is_byte_for_byte_the_unknown_major_refusal() {
    let unknown = envelope_for(
        &cmd(),
        &ApiReply::Failed(api_error(
            "not_found",
            "unknown_major",
            "\"demo.show_thing\"@1: unknown major",
            ApiStage::Resolve,
        )),
    );
    let expected = encoded(&unknown);
    assert_eq!(encoded(&hidden_as_unknown_major(&cmd())), expected);
    for stage in ApiStage::ALL {
        for detail in ["demo.show_thing@1: hidden major; only replay may invoke it", "", "other"] {
            let hidden =
                envelope_for(&cmd(), &ApiReply::Failed(api_error("hidden_command", "hidden_major", detail, *stage)));
            assert_eq!(encoded(&hidden), expected, "{stage} {detail:?}");
        }
    }
    // The name is shown as the bus shows it, and the major is the requested one.
    let other = CommandId::parse("a.b_c@12").unwrap();
    let hidden =
        envelope_for(&other, &ApiReply::Failed(api_error("hidden_command", "hidden_major", "x", ApiStage::Resolve)));
    assert_eq!(hidden.error.map(|e| e.message), Some("\"a.b_c\"@12: unknown major".to_string()));
}

// ------------------------------------------------------------------ AC-3

fn details(env: &Envelope) -> &Obj {
    obj(env.error.as_ref().and_then(|e| e.details.as_ref()).unwrap())
}

/// AC-3: `bus_origin` per stage; a handler's `not_found/unknown_command` is not `pipeline`.
#[test]
fn ac3_bus_origin_per_stage() {
    let expect = [
        ("resolve", "pipeline"),
        ("validate", "pipeline"),
        ("authorise", "bus"),
        ("handler", "handler"),
        ("commit", "bus"),
        ("session", "bus"),
    ];
    for (stage, origin) in expect {
        let stage = ApiStage::parse(stage).unwrap();
        let env = envelope_for(&cmd(), &ApiReply::Failed(api_error("validation", "schema", "x", stage)));
        assert_eq!(st(details(&env), "bus_origin"), origin, "{stage}");
        assert_eq!(bus_origin(stage), origin, "{stage}");
    }
    // Every other stage is `bus` too.
    for stage in ["precondition", "route", "effects"] {
        assert_eq!(bus_origin(ApiStage::parse(stage).unwrap()), "bus", "{stage}");
    }
    let env = envelope_for(
        &cmd(),
        &ApiReply::Failed(api_error("not_found", "unknown_command", "other.cmd@1", ApiStage::Handler)),
    );
    let d = details(&env);
    assert_eq!((st(d, "bus_code"), st(d, "bus_reason")), ("not_found", "unknown_command"));
    assert_ne!(st(d, "bus_origin"), "pipeline", "AUTO.CLI.DISPATCH's absence check keys on bus_origin pipeline");
}

// ------------------------------------------------------------------ AC-4

/// AC-4: `Cancel` and `Wait` are refused with `no_live_job` and call nothing; a closed
/// transport sends nothing and the tracker reports `not_run`.
#[test]
fn ac4_cancel_and_wait_are_refused_without_a_call() {
    let (log, port) = FakePort::new(vec![]);
    let mut t = tracker(port);
    t.transport_mut().hold = true;
    let id = run(&mut t, json("{}"));
    let Some(State::Accepted { .. }) = t.state(id) else { panic!("not accepted: {:?}", t.state(id)) };
    assert_eq!(log.lock().unwrap().calls.len(), 1);

    assert_eq!(t.request_cancel(id).unwrap(), CancelStatus::Sent);
    t.reattach(id, MS200).unwrap();
    let before = t.transport().seen.len();
    t.pump(&mut ());
    let refusals: Vec<(Uuid, u32, String)> = t.transport().seen[before..]
        .iter()
        .map(|e| match e {
            BusEvent::Refused { key, attempt, reason } => (*key, *attempt, reason.clone()),
            other => panic!("not a refusal: {other:?}"),
        })
        .collect();
    assert_eq!(refusals.len(), 2, "{refusals:?}");
    for (key, attempt, reason) in &refusals {
        assert_eq!(reason, NO_LIVE_JOB);
        assert_eq!(*attempt, 0);
        assert_ne!(*key, t.key(id).unwrap(), "cancel and wait use their own keys");
    }
    assert_ne!(refusals[0].0, refusals[1].0);
    assert_eq!(log.lock().unwrap().calls.len(), 1, "the port's call count is unchanged");

    // The held final state still ends the invocation.
    t.transport_mut().hold = false;
    t.pump(&mut ());
    assert_eq!(t.report(id).unwrap().envelope.outcome, Outcome::Computed);
}

/// REQ-008 directly: the refusal names the request's own key and attempt.
#[test]
fn refusal_echoes_key_and_attempt() {
    let (log, port) = FakePort::new(vec![]);
    let mut tr = transport(port);
    let key = Uuid::parse("01901234-5678-7000-8000-000000000042").unwrap();
    for purpose in [Purpose::Cancel, Purpose::Wait] {
        let req = BusRequest { purpose, key, attempt: 3, command: cmd(), params: json("{}") };
        tr.send(&req).unwrap();
        assert_eq!(tr.recv(), Some(BusEvent::Refused { key, attempt: 3, reason: NO_LIVE_JOB.to_string() }));
        assert_eq!(tr.recv(), None);
    }
    assert!(log.lock().unwrap().calls.is_empty());
}

#[test]
fn ac4_closed_transport_sends_nothing() {
    let (log, port) = FakePort::new(vec![]);
    let mut t = tracker(port);
    t.transport_mut().inner.close();
    assert!(t.transport().inner.is_closed());
    let id = run(&mut t, json("{}"));
    assert!(log.lock().unwrap().calls.is_empty(), "the port is not called");
    let report = t.report(id).unwrap();
    assert_eq!(report.envelope.outcome, Outcome::NotRun);
    assert_eq!(report.reason, Reason::NotSent);

    let mut tr = transport(FakePort::new(vec![]).1);
    tr.close();
    for purpose in [Purpose::Submit, Purpose::Cancel, Purpose::Wait] {
        let req = BusRequest { purpose, key: Uuid::from_u128(1), attempt: 0, command: cmd(), params: json("{}") };
        assert_eq!(tr.send(&req), Err(SendError::NotSent), "{purpose:?}");
    }
    assert_eq!(tr.recv(), None);
}

/// The queue is bounded: once full, `send` returns `NotSent` without calling the port.
#[test]
fn full_queue_sends_nothing() {
    let (log, port) = FakePort::new(vec![]);
    let mut tr = transport(port);
    let req =
        BusRequest { purpose: Purpose::Wait, key: Uuid::from_u128(1), attempt: 0, command: cmd(), params: json("{}") };
    for _ in 0..MAX_QUEUED_EVENTS {
        tr.send(&req).unwrap();
    }
    assert_eq!(tr.queued(), MAX_QUEUED_EVENTS);
    let submit = BusRequest { purpose: Purpose::Submit, ..req.clone() };
    assert_eq!(tr.send(&submit), Err(SendError::NotSent));
    assert_eq!(tr.send(&req), Err(SendError::NotSent));
    assert!(log.lock().unwrap().calls.is_empty());
    assert!(tr.recv().is_some());
    assert!(tr.recv().is_some());
    tr.send(&submit).unwrap();
    assert_eq!(log.lock().unwrap().calls.len(), 1);
}

// ------------------------------------------------------------------ AC-5

/// AC-5: the transport reports what the port reports, and forwards `confirm_human` once.
#[test]
fn ac5_principal_kind_and_confirm_human_pass_through() {
    let (log, port) = FakePort::new(vec![]);
    let mut t = tracker(port);
    assert_eq!(t.transport().inner.principal_kind(), PrincipalKind::Script);
    t.transport_mut().inner.confirm_human().unwrap();
    assert_eq!(t.transport().inner.principal_kind(), PrincipalKind::Human);
    let l = log.lock().unwrap();
    assert_eq!(l.confirms, 1, "confirm_human reached the port exactly once");
    assert_eq!(l.kind_reads, 2);
    drop(l);
    assert_eq!(
        t.transport().inner.channel(),
        ChannelInfo { kind: PortKind::Cli, identity: "user-a".to_string(), session: "cli#1".to_string() }
    );
}

/// A refused upgrade (no presence provider) is returned unchanged and the kind stays `script`.
#[test]
fn refused_upgrade_fails_closed() {
    let (log, mut port) = FakePort::new(vec![]);
    port.confirm_ok = false;
    let mut tr = transport(port);
    let err = tr.confirm_human().unwrap_err();
    assert_eq!((err.code(), err.reason(), err.detail()), ("upgrade_refused", "declined", "no_presence_provider"));
    assert_eq!(tr.principal_kind(), PrincipalKind::Script);
    assert_eq!(log.lock().unwrap().confirms, 1);
}

// ------------------------------------------------------------------ the table

/// Every bus code of the catalogue other than `cancelled` has a table row, and every row names
/// a code and reason the catalogue holds, so no current bus code falls to `internal_error`
/// through REQ-007.
#[test]
fn table_matches_the_bus_error_catalogue() {
    let cat = read_json("schema/bus-errors.json");
    let codes = obj(&obj(&cat)["codes"]);
    for code in codes.keys() {
        if code == CANCELLED {
            assert!(BUS_ERROR_TABLE.iter().all(|(c, _, _)| *c != CANCELLED));
            continue;
        }
        assert!(BUS_ERROR_TABLE.iter().any(|(c, r, _)| c == code && r.is_none()), "no catch-all row for {code}");
    }
    for (code, reason, _) in BUS_ERROR_TABLE {
        let Some(CValue::Arr(reasons)) = codes.get(*code) else { panic!("{code} is not a bus code") };
        if let Some(r) = reason {
            assert!(reasons.contains(&CValue::Str((*r).to_string())), "{code}/{r} is not a bus reason");
        }
    }
    assert_eq!(error_code_for("future_code", "x"), ErrorCode::InternalError);
    assert_eq!(error_code_for("validation", "unit_mismatch"), ErrorCode::UnitError);
}

/// The `policy` rows agree with the root gate's own mapping (`RootError::error_code`).
#[test]
fn policy_rows_agree_with_the_root_gate() {
    let errors = [
        RootError::RawPathRefused { kind: RootKind::Script, param: "file".into() },
        RootError::BadHandle { param: "file".into() },
        RootError::RootConfigHidden { kind: RootKind::Agent, command: CMD.into() },
        RootError::NestedPathLike { command: CMD.into() },
    ];
    for e in &errors {
        assert_eq!(error_code_for(e.bus_code(), e.bus_reason()), e.error_code(), "{e:?}");
    }
    assert_eq!(error_code_for("policy", "outside_root"), ErrorCode::RootEscape);
}
