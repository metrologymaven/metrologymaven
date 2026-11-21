//! AUTO.MCP.PROJ AC-3 (and the bus side of AC-4) through the tool-call adapter: a fake bus,
//! and a bus made of the real CORE.BUS.POL.PROJ authorise stage and filing, which files a real
//! pending request. Refusals are checked against the v3 bus error catalogue, and the
//! `request.*` tools are checked with real parameters and a real result.
#![cfg(not(target_family = "wasm"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{conforms, defs_of, json, projection_of, read};
use fid_bus::error::{BusError, Code};
use fid_bus::handler::{Event, ValidParams};
use fid_bus::policy::projection::{
    authorise, command_defs, list_requests, Decision, EntryOrigin, RecordContext, RequestBook, RequestId,
};
use fid_bus::principal::{
    Channel, ChannelKind, Identity, JournalAppend, OsUser, Principal, PrincipalId, PrincipalKind, SessionId,
    SessionKey, SessionTable,
};
use fid_canon::{parse, CValue, Digest};
use fid_journal::{DraftBody, EntryDraft, EntryTime, Manifests};
use fid_mcp::adapter::PRINCIPAL_KIND;
use fid_mcp::tools::tool_for;
use fid_mcp::{call_tool, AgentSession, AuditSink, BusCall, McpBus, Refusal, Severity, ToolKind, ToolTable, Violation};
use fid_registry::CommandDef;
use fid_surface::{project, Envelope, ErrorCode, Outcome};

const CALLS: &str = "fixtures/mcp/calls.json";
const HEAD: Digest = Digest([0x77; 32]);

fn table() -> ToolTable {
    ToolTable::from_projection(&project(&read(CALLS)).unwrap()).unwrap()
}

fn rid(n: u8) -> RequestId {
    RequestId::parse(&format!("0193a5b0-1c2d-7e3f-8a4b-0000000000{n:02x}")).unwrap()
}

/// Records every violation the adapter reports.
#[derive(Default)]
struct Audit(Vec<Violation>);

impl AuditSink for Audit {
    fn violation(&mut self, v: Violation) {
        self.0.push(v);
    }
}

// ------------------------------------------------------------------ the fake bus

/// What a bus saw of one call: channel, the principal kind it derives for that channel,
/// command and parameters.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    channel: String,
    principal_kind: &'static str,
    command: String,
    params: CValue,
}

fn seen(call: &BusCall<'_>) -> Seen {
    let kind = ChannelKind::from_wire(call.channel).unwrap();
    Seen {
        channel: call.channel.to_string(),
        principal_kind: kind.base_kind().as_str(),
        command: call.command.as_str().to_string(),
        params: call.params.clone(),
    }
}

/// Records every call and answers with `reply`.
struct FakeBus {
    calls: Vec<Seen>,
    reply: fn(&BusCall<'_>) -> Envelope,
}

impl McpBus for FakeBus {
    fn invoke(&mut self, call: &BusCall<'_>) -> Envelope {
        self.calls.push(seen(call));
        (self.reply)(call)
    }
}

/// A well-behaved bus: request-only commands are filed, others run.
fn honest(call: &BusCall<'_>) -> Envelope {
    let id = call.command.as_str();
    if id == "demo.release_plan@1" {
        Envelope { request_id: Some(rid(1).to_string()), ..Envelope::bare(id, Outcome::RequestFiled) }
    } else if id == "demo.set_offset@1" {
        Envelope {
            journal_entry_hash: Some(format!("sha256:{}", "cd".repeat(32))),
            ..Envelope::bare(id, Outcome::Committed)
        }
    } else {
        Envelope::computed(id, json("{}"))
    }
}

fn fake(reply: fn(&BusCall<'_>) -> Envelope) -> FakeBus {
    FakeBus { calls: Vec::new(), reply }
}

/// AC-3: the W3 tool reaches the bus on channel `mcp` as an agent and returns `request_filed`
/// with a request ID.
#[test]
fn ac3_fake_bus_gets_mcp_agent_and_returns_request_filed() {
    let t = table();
    assert_eq!(t.get("demo_release_plan").map(|(_, k)| k), Some(ToolKind::RequestOnly));
    let session = AgentSession::new("session-1");
    assert_eq!(session.principal_kind(), PRINCIPAL_KIND);
    let mut bus = fake(honest);
    let args = json(r#"{"plan":"P-7"}"#);
    let env = call_tool(&t, &session, &mut bus, &mut Audit::default(), "demo_release_plan", &args).unwrap();
    assert_eq!(env.outcome, Outcome::RequestFiled);
    assert_eq!(env.request_id.as_deref(), Some(rid(1).to_string().as_str()));
    assert_eq!(
        bus.calls,
        vec![Seen {
            channel: "mcp".into(),
            principal_kind: "agent",
            command: "demo.release_plan@1".into(),
            params: args.clone()
        }]
    );
    // A direct W2 tool goes through the same path, unchanged.
    let w2 = json(r#"{"offset":{"unit":"mm","value":"1.5"}}"#);
    let env = call_tool(&t, &session, &mut bus, &mut Audit::default(), "demo_set_offset", &w2).unwrap();
    assert_eq!(env.outcome, Outcome::Committed);
    assert_eq!(bus.calls[1].principal_kind, "agent");
}

/// Tools with no definition (A0, M1) or unknown names never reach the bus.
#[test]
fn unknown_tools_are_refused_before_the_bus() {
    let t = table();
    let names: Vec<&str> = t.names().collect();
    assert_eq!(names, ["demo_release_plan", "demo_set_offset", "demo_show_plan"]);
    let session = AgentSession::new("s");
    let mut bus = fake(honest);
    for name in ["demo_erase_disk", "demo_move_axis", "request_reject_request", "", "Demo_Show_Plan", "x\u{202e}y"] {
        let e = call_tool(&t, &session, &mut bus, &mut Audit::default(), name, &json("{}")).unwrap_err();
        assert_eq!((e.code(), e.reason()), ("not_found", "unknown_command"), "{name:?}");
        assert!(!e.detail().contains('\u{202e}'));
    }
    for args in ["[]", "null", "\"x\"", "1"] {
        let e = call_tool(&t, &session, &mut bus, &mut Audit::default(), "demo_show_plan", &json(args)).unwrap_err();
        assert_eq!((e.code(), e.reason()), ("validation", "schema"), "{args}");
    }
    assert!(bus.calls.is_empty());
}

/// A bus answer that breaks an invariant is reported for audit at error severity and refused,
/// never passed on. The check detects; the bus is the only gate.
#[test]
fn invariant_violations_are_refused() {
    let t = table();
    let session = AgentSession::new("s");
    let args = json(r#"{"plan":"P"}"#);
    let replies: [fn(&BusCall<'_>) -> Envelope; 4] = [
        // A request-only command that ran.
        |c| Envelope {
            journal_entry_hash: Some(format!("sha256:{}", "ab".repeat(32))),
            ..Envelope::bare(c.command.as_str(), Outcome::Committed)
        },
        |c| Envelope::computed(c.command.as_str(), json("{}")),
        // An answer for another command.
        |_| Envelope { request_id: Some("r".into()), ..Envelope::bare("demo.show_plan@1", Outcome::RequestFiled) },
        // An invalid envelope: request_filed without a request ID.
        |c| Envelope::bare(c.command.as_str(), Outcome::RequestFiled),
    ];
    for reply in replies {
        let mut bus = fake(reply);
        let mut audit = Audit::default();
        let e = call_tool(&t, &session, &mut bus, &mut audit, "demo_release_plan", &args).unwrap_err();
        assert_eq!((e.code(), e.reason()), ("internal", "invariant_violated"), "{e}");
        assert_eq!(e.envelope_code(), ErrorCode::InternalError);
        assert_eq!(audit.0.len(), 1, "{e}");
        let v = &audit.0[0];
        assert_eq!(v.severity, Severity::Error);
        assert_eq!(
            (v.session.as_str(), v.tool.as_str(), v.command.as_str()),
            ("s", "demo_release_plan", "demo.release_plan@1")
        );
        assert_eq!(v.detail, e.detail());
        assert_eq!(
            v.envelope,
            reply(&BusCall { channel: "mcp", session: &session, command: &v.command, params: &args })
        );
    }
    // Outcomes that show nothing ran are passed on for a request-only tool, with no audit event.
    let mut audit = Audit::default();
    let mut bus = fake(|c| Envelope::failed(c.command.as_str(), ErrorCode::PolicyRefused, "no", None));
    let env = call_tool(&t, &session, &mut bus, &mut audit, "demo_release_plan", &args).unwrap();
    assert_eq!(env.outcome, Outcome::Failed);
    let mut bus = fake(|c| Envelope::bare(c.command.as_str(), Outcome::NotRun));
    let env = call_tool(&t, &session, &mut bus, &mut audit, "demo_release_plan", &args).unwrap();
    assert_eq!(env.outcome, Outcome::NotRun);
    assert!(audit.0.is_empty());
}

/// Every refusal is a code and reason of the v3 bus error catalogue, and of this build's.
#[test]
fn refusals_map_to_the_v3_catalogue() {
    let raw = read("fixtures/core/bus/pipe/err/bus-errors.v3.json");
    let v3 = fid_bus::error::parse_catalog(&raw).unwrap();
    let all = [
        Refusal::UnknownTool { detail: "d".into() },
        Refusal::BadArguments { detail: "d".into() },
        Refusal::InvariantViolated { detail: "d".into() },
    ];
    assert_eq!(all.len(), Refusal::PAIRS.len());
    for (r, (code, reason)) in all.iter().zip(Refusal::PAIRS) {
        assert_eq!((r.code(), r.reason()), (code, reason));
        assert!(v3.get(code).is_some_and(|rs| rs.contains(reason)), "{code}/{reason} is not in bus-errors v3");
        let e = BusError::from_wire(code, reason, r.detail()).unwrap();
        assert_eq!(e.to_value(), r.to_value());
    }
}

// ------------------------------------------------------------------ the POL.PROJ bus

struct TestChannel {
    kind: ChannelKind,
    identity: Identity,
    session: SessionKey,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(self.identity.clone())
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

#[derive(Default)]
struct Journal {
    drafts: Vec<EntryDraft>,
}

impl JournalAppend for Journal {
    fn append(&mut self, draft: &EntryDraft) -> Result<(), BusError> {
        self.drafts.push(draft.clone());
        Ok(())
    }
}

fn envelope_code(c: Code) -> ErrorCode {
    match c {
        Code::Policy => ErrorCode::PolicyRefused,
        Code::Validation => ErrorCode::ValidationError,
        Code::NotFound => ErrorCode::NotFound,
        _ => ErrorCode::InternalError,
    }
}

/// The real authorise stage and filing of CORE.BUS.POL.PROJ behind the adapter. A call the
/// projection lets run is answered `not_run`: running it is CORE.BUS.PIPE.STAGE, not built.
struct PolProjBus {
    table: SessionTable,
    defs: Vec<CommandDef>,
    book: RequestBook,
    journal: Journal,
    filed: u8,
    principals: Vec<Principal>,
    ran: Vec<String>,
}

impl PolProjBus {
    fn new() -> PolProjBus {
        PolProjBus {
            table: SessionTable::new(),
            defs: defs_of(&read(CALLS)),
            book: RequestBook::new(),
            journal: Journal::default(),
            filed: 0,
            principals: Vec::new(),
            ran: Vec::new(),
        }
    }

    fn channel(&mut self, call: &BusCall<'_>) -> TestChannel {
        let label = call.session.label().to_string();
        let session = self.table.open_session(SessionId::new(&label).unwrap()).unwrap();
        let mut identity = Identity::new(PrincipalId::new(&label).unwrap());
        identity.os_user = Some(OsUser::new(&label).unwrap());
        TestChannel { kind: ChannelKind::from_wire(call.channel).unwrap(), identity, session }
    }

    /// Folds the journal into the request book.
    fn fold(&mut self) {
        let mut book = RequestBook::new();
        for d in &self.journal.drafts {
            let DraftBody::Command(c) = &d.body else { panic!("not a command entry") };
            let events: Vec<Event> = c.events.iter().map(|e| Event::from_value(e.clone()).unwrap()).collect();
            book.apply_entry(&EntryOrigin::from_draft(d).unwrap(), &events).unwrap();
        }
        self.book = book;
    }
}

impl McpBus for PolProjBus {
    fn invoke(&mut self, call: &BusCall<'_>) -> Envelope {
        let id = call.command.as_str();
        let ch = self.channel(call);
        let def = self.defs.iter().find(|d| format!("{}@{}", d.id, d.major) == id).unwrap();
        let view = self.book.view(HEAD);
        let decided = authorise(&self.table, &ch, def, &ValidParams::for_tests(call.params.clone()), None, &view);
        let failed = |e: BusError| Envelope::failed(id, envelope_code(e.code()), &e.to_string(), Some(e.to_value()));
        let a = match decided {
            Ok(a) => a,
            Err(e) => return failed(e),
        };
        self.principals.push(a.principal().clone());
        let env = match a.decision() {
            Decision::File(f) => {
                self.filed += 1;
                let ctx = RecordContext {
                    stream: "record".into(),
                    branch: "main".into(),
                    manifests: Manifests { registry: Digest([0x11; 32]), kernel: Digest([0x22; 32]) },
                };
                let time = EntryTime { utc: "2026-11-20T10:00:00Z".into(), mono_ns: 1_000, sync: "ntp".into() };
                match f.commit(rid(self.filed), &view, time, &ctx, &mut self.journal) {
                    Ok(r) => Envelope { request_id: Some(r.to_string()), ..Envelope::bare(id, Outcome::RequestFiled) },
                    Err(e) => failed(e),
                }
            }
            Decision::Run => {
                self.ran.push(id.to_string());
                Envelope::bare(id, Outcome::NotRun)
            }
            Decision::RunApproved(_) => panic!("the adapter never asks for an approval"),
        };
        self.fold();
        env
    }
}

/// AC-3 against the real POL.PROJ: one `request.file_request@1` entry by the agent, for the
/// W3 command, and nothing ran. The agent then lists its request with the real
/// `request.list_requests@1` (real result, checked against that tool's output schema).
#[test]
fn ac3_pol_proj_files_a_real_pending_request() {
    let t = table();
    let session = AgentSession::new("agent-7");
    let mut bus = PolProjBus::new();
    let env = call_tool(&t, &session, &mut bus, &mut Audit::default(), "demo_release_plan", &json(r#"{"plan":"P-7"}"#))
        .unwrap();
    assert_eq!(env.outcome, Outcome::RequestFiled);
    assert_eq!(env.request_id.as_deref(), Some(rid(1).to_string().as_str()));
    assert!(bus.ran.is_empty(), "a request-only command ran: {:?}", bus.ran);
    assert_eq!(bus.journal.drafts.len(), 1);
    let d = &bus.journal.drafts[0];
    assert_eq!(d.principal.kind, PrincipalKind::Agent);
    let DraftBody::Command(c) = &d.body else { panic!("not a command entry") };
    assert_eq!((c.id.as_str(), c.major), ("request.file_request", 1));
    assert!(matches!(&c.args, CValue::Obj(o) if o.get("command") == Some(&CValue::Str("demo.release_plan@1".into()))));

    // The real list_requests result conforms to the generated request_list_requests tool.
    let agent = bus.principals[0].clone();
    assert_eq!(agent.kind().as_str(), PRINCIPAL_KIND);
    let result = list_requests(&bus.book, &agent, &json("{}")).unwrap();
    let CValue::Obj(r) = &result else { panic!("result must be an object") };
    assert!(matches!(r.get("requests"), Some(CValue::Arr(a)) if a.len() == 1));
    let list = real_tool("request.list_requests").unwrap();
    assert_eq!((list.name.as_str(), list.kind), ("request_list_requests", ToolKind::Direct));
    assert!(list.annotations.read_only_hint);
    assert!(conforms(&list.input_schema, &json("{}")));
    assert!(conforms(&list.input_schema, &json(r#"{"state":"pending"}"#)));
    let CValue::Obj(out) = &list.output_schema else { panic!() };
    let Some(CValue::Obj(props)) = out.get("properties") else { panic!() };
    assert!(conforms(&props["result"], &result), "{result:?}");
    let env = Envelope::computed("request.list_requests@1", result);
    assert!(conforms(&list.output_schema, &parse(&env.encode().unwrap()).unwrap()));

    // Real withdraw parameters conform to the generated request_withdraw_request tool.
    let withdraw = real_tool("request.withdraw_request").unwrap();
    assert_eq!(withdraw.kind, ToolKind::Direct);
    let params = CValue::Obj([("request".to_string(), CValue::Str(rid(1).to_string()))].into_iter().collect());
    assert!(conforms(&withdraw.input_schema, &params));
    assert!(!conforms(&withdraw.input_schema, &json(r#"{"request":"x","principal":"human"}"#)));
    // Rejecting is human-only and never on MCP; filing is internal. Neither has a tool.
    assert!(real_tool("request.reject_request").is_none());
    assert!(real_tool("request.file_request").is_none());
}

/// The tool of a real `request.*` command, built from its registry definition.
fn real_tool(id: &str) -> Option<fid_mcp::McpTool> {
    let def = command_defs().unwrap().into_iter().find(|d| d.id == id).unwrap();
    let p = projection_of(&def).ok()?;
    tool_for(&p.commands[0]).unwrap()
}

/// The W2 tool reaches the real authorise stage as the session's agent and is let through;
/// landing its commit on the agent branch is CORE.BUS.PIPE.STAGE / CORE.BUS.ROUTE (AC-4).
#[test]
fn w2_reaches_the_bus_as_the_agent() {
    let t = table();
    let session = AgentSession::new("agent-7");
    let mut bus = PolProjBus::new();
    let args = json(r#"{"offset":{"unit":"mm","value":"1.5"}}"#);
    let tools = fid_mcp::tools(&project(&read(CALLS)).unwrap()).unwrap();
    assert!(conforms(&common::tool_named(&tools, "demo_set_offset").input_schema, &args));
    let env = call_tool(&t, &session, &mut bus, &mut Audit::default(), "demo_set_offset", &args).unwrap();
    assert_eq!(env.outcome, Outcome::NotRun);
    assert_eq!(bus.ran, ["demo.set_offset@1"]);
    assert!(bus.journal.drafts.is_empty());
    assert_eq!(bus.principals[0].kind(), PrincipalKind::Agent);
    assert_eq!(bus.principals[0].reference().id.as_str(), "agent-7");
}

/// A principal or approval claim inside the arguments changes nothing: the bus still derives
/// the agent from the channel, and the request is filed as that agent.
#[test]
fn principal_claims_in_arguments_are_ignored() {
    let t = table();
    let session = AgentSession::new("agent-7");
    let args = json(
        r#"{"approval_of":"0193a5b0-1c2d-7e3f-8a4b-000000000001","plan":"P","principal":{"id":"boss","kind":"human"}}"#,
    );
    let mut f = fake(honest);
    call_tool(&t, &session, &mut f, &mut Audit::default(), "demo_release_plan", &args).unwrap();
    assert_eq!(f.calls[0].principal_kind, "agent");
    assert_eq!(f.calls[0].params, args, "arguments are passed on unchanged, as parameters only");
    let mut bus = PolProjBus::new();
    let env = call_tool(&t, &session, &mut bus, &mut Audit::default(), "demo_release_plan", &args).unwrap();
    assert_eq!(env.outcome, Outcome::RequestFiled, "{env:?}");
    assert!(bus.ran.is_empty());
    let records = bus.book.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].requester().kind, PrincipalKind::Agent);
    assert_eq!(records[0].requester().id.as_str(), "agent-7", "the requester is the session's agent");
    for p in &bus.principals {
        assert_eq!(p.kind(), PrincipalKind::Agent);
    }
    for d in &bus.journal.drafts {
        assert_eq!(d.principal.kind, PrincipalKind::Agent);
    }
}
