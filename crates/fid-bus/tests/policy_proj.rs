//! Acceptance criteria of spec node CORE.BUS.POL.PROJ (AC-1 .. AC-6; AC-7 in part), against
//! `fixtures/core/bus/pol/proj/registry.json` and `cases.json`, written by the independent
//! reference `fixtures/core/bus/pol/proj/make_cases.py` (outcomes hand-labelled from the spec,
//! commitments computed with Python's hashlib).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code};
use fid_bus::handler::{run_handler, CancelToken, Event, Handler, HandlerCtx, HandlerSet, StateView, ValidParams};
use fid_bus::policy::class::authorise_class;
use fid_bus::policy::projection::{
    authorise, authorise_projection, command_defs, is_mcp_channel, list_requests, mcp_projection, params_commitment,
    Approval, Decision, Filing, Projected, RecordContext, RejectHandler, RequestBook, RequestId, RequestRecord,
    RequestState, WithdrawHandler, FILE_REQUEST, LIST_REQUESTS, MAX_REASON_CHARS, PARAMS_TAG, REJECT_REQUEST,
    REQUEST_APPROVED, REQUEST_FILED, REQUEST_KIND, REQUEST_REJECTED, REQUEST_WITHDRAWN, WITHDRAW_REQUEST,
};
use fid_bus::principal::{
    Channel, ChannelKind, Identity, JournalAppend, OsUser, PrincipalId, PrincipalKind, PrincipalRef, SessionId,
    SessionKey, SessionTable,
};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_journal::{
    build, AiAssistance, DraftBody, EntryDraft, EntryError, EntryTime, Field, Manifests, Salt, SaltSource, GENESIS_PREV,
};
use fid_registry::{check_schema, Catalogue, CommandDecl, CommandDef, Projection};
use fid_types::{Determinism, SideEffect};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

type Obj = BTreeMap<String, CValue>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(rel: &str) -> CValue {
    let bytes = std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    parse(&bytes).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> &'a [CValue] {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("{k}: {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: {other:?}"),
    }
}

fn json(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

fn canon(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).unwrap()
}

fn cases() -> CValue {
    read_json("fixtures/core/bus/pol/proj/cases.json")
}

fn registry() -> Vec<CommandDef> {
    let r = read_json("fixtures/core/bus/pol/proj/registry.json");
    arr(obj(&r), "commands").iter().map(|c| CommandDef::from_value(c.clone()).unwrap()).collect()
}

fn def(id: &str) -> CommandDef {
    let (name, major) = id.rsplit_once('@').unwrap();
    let all = registry().into_iter().chain(command_defs().unwrap());
    all.into_iter().find(|d| d.id == name && d.major.to_string() == major).unwrap_or_else(|| panic!("{id}"))
}

fn assert_err(r: Result<impl std::fmt::Debug, BusError>, code: Code, reason: &str) {
    let e = r.unwrap_err();
    assert_eq!((e.code(), e.reason()), (code, reason), "{e}: {}", e.detail().as_untrusted_str());
}

// ------------------------------------------------------------------ channels and principals

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

fn pid(s: &str) -> PrincipalId {
    PrincipalId::new(s).unwrap()
}

fn channel(table: &mut SessionTable, kind: ChannelKind, subject: &str) -> TestChannel {
    let mut identity = Identity::new(pid(subject));
    identity.os_user = Some(OsUser::new(subject).unwrap());
    if matches!(kind, ChannelKind::ServerSession | ChannelKind::Internal) {
        identity.acting_for = Some(PrincipalRef { kind: PrincipalKind::Human, id: pid("trigger") });
    }
    let session = table.open_session(SessionId::new(&format!("{}-{subject}", kind.as_str())).unwrap()).unwrap();
    TestChannel { kind, identity, session }
}

fn kind_of(wire: &str) -> ChannelKind {
    ChannelKind::from_wire(wire).unwrap_or_else(|| panic!("channel {wire}"))
}

// ------------------------------------------------------------------ journal

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

struct FixedSalts;

impl SaltSource for FixedSalts {
    fn salt(&mut self, _field: Field) -> Result<Salt, EntryError> {
        Ok(Salt([0x5a; 16]))
    }
}

fn record_ctx() -> RecordContext {
    RecordContext {
        stream: "record".into(),
        branch: "main".into(),
        manifests: Manifests { registry: Digest([0x11; 32]), kernel: Digest([0x22; 32]) },
    }
}

fn time() -> EntryTime {
    EntryTime { utc: "2026-11-20T10:00:00Z".into(), mono_ns: 1_000, sync: "ntp".into() }
}

const HEAD: Digest = Digest([0x77; 32]);

fn draft_events(d: &EntryDraft) -> Vec<Event> {
    let DraftBody::Command(c) = &d.body else { panic!("not a command entry") };
    c.events.iter().map(|e| Event::from_value(e.clone()).unwrap()).collect()
}

/// A UUIDv7 request ID ending in `n`.
fn rid(n: u8) -> RequestId {
    RequestId::parse(&format!("0193a5b0-1c2d-7e3f-8a4b-0000000000{n:02x}")).unwrap()
}

/// The state every call runs against: the request book plus its journal.
struct World {
    table: SessionTable,
    book: RequestBook,
    journal: Journal,
}

impl World {
    fn new() -> World {
        World { table: SessionTable::new(), book: RequestBook::new(), journal: Journal::default() }
    }

    fn channel(&mut self, kind: ChannelKind, subject: &str) -> TestChannel {
        channel(&mut self.table, kind, subject)
    }

    fn authorise(
        &self,
        ch: &TestChannel,
        id: &str,
        p: &CValue,
        approval_of: Option<&RequestId>,
    ) -> Result<Decision, BusError> {
        let view = self.book.view(HEAD);
        authorise(&self.table, ch, &def(id), p, approval_of, &view).map(|a| a.into_parts().1)
    }

    /// Files `p` for command `id` over `ch` under request ID `n` and folds the entry.
    fn file(&mut self, ch: &TestChannel, id: &str, p: &CValue, n: u8) -> RequestId {
        let Decision::File(filing) = self.authorise(ch, id, p, None).unwrap() else { panic!("{id} did not file") };
        self.commit_filing(&filing, rid(n)).unwrap()
    }

    fn commit_filing(&mut self, filing: &Filing, id: RequestId) -> Result<RequestId, BusError> {
        let view = self.book.view(HEAD);
        let got = filing.commit(id, &view, time(), &record_ctx(), &mut self.journal)?;
        let events = draft_events(self.journal.drafts.last().unwrap());
        self.book.apply_entry(&events).unwrap();
        Ok(got)
    }

    fn record(&self, id: RequestId) -> RequestRecord {
        self.book.get(&id).unwrap().clone()
    }

    fn list(&mut self, ch: &TestChannel, p: &CValue) -> Result<CValue, BusError> {
        let principal = self.table.derive_principal(ch)?;
        list_requests(&self.book, &principal, p)
    }

    /// Runs a request command's handler through the bus handler contract.
    fn run(&self, ch: &TestChannel, id: &str, p: &CValue) -> Result<Vec<Event>, BusError> {
        let mut set = HandlerSet::new();
        set.register(def(&format!("{WITHDRAW_REQUEST}@1")), Box::new(WithdrawHandler)).unwrap();
        set.register(def(&format!("{REJECT_REQUEST}@1")), Box::new(RejectHandler)).unwrap();
        let ctx = HandlerCtx::derive(&self.table, ch, 0, CancelToken::never())?;
        let view = self.book.view(HEAD);
        run_handler(&set, id, &view, &ValidParams::for_tests(p.clone()), &ctx).map(|c| c.events().to_vec())
    }
}

fn filed_params() -> CValue {
    let c = cases();
    obj(&c).get("filed").map(|f| obj(f).get("params").unwrap().clone()).unwrap()
}

const REQUEST_CMD: &str = "test.run_request@1";
const HUMAN_CMD: &str = "test.adopt_human@1";

// ------------------------------------------------------------------ projection (REQ-001, REQ-002)

/// The projection check gives the fixture's hand-labelled outcome for every command of the
/// test registry on every channel kind.
#[test]
fn projection_matches_the_fixture_on_every_channel() {
    let c = cases();
    let rows = arr(obj(&c), "projection");
    assert_eq!(rows.len(), registry().len() * ChannelKind::ALL.len());
    for row in rows {
        let o = obj(row);
        let kind = kind_of(st(o, "channel"));
        let d = def(st(o, "command"));
        let got = match authorise_projection(kind, &d) {
            Ok(Projected::Run) => "run".to_string(),
            Ok(Projected::PendingRequest) => "pending".to_string(),
            Err(e) => format!("{}/{}", e.code(), e.reason()),
        };
        assert_eq!(got, st(o, "expect"), "{} on {}", d.id, kind);
    }
    let mcp: Vec<ChannelKind> = ChannelKind::ALL.iter().copied().filter(|k| is_mcp_channel(*k)).collect();
    assert_eq!(mcp, [ChannelKind::Mcp, ChannelKind::ServerMcp]);
}

fn class_def(class: &str, projection: &str, human_only: bool) -> CommandDef {
    let mut v = obj(&json(
        r#"{"id":"test.classed","major":1,"title":"t","description":"d","params_schema":{"type":"object"},
"result_schema":{"type":"object"},"side_effect":"W1","mcp_projection":"exposed","determinism":"D0",
"idempotency":"none","fs_effect":"none","undo":"head_move","preconditions":[],"effects":["TestDone"],
"algorithms":[],"exposure":["mcp"],"lifecycle":{"state":"active","since":"2026-11-20"},"web":"available"}"#,
    ))
    .clone();
    v.insert("side_effect".into(), CValue::Str(class.into()));
    v.insert("mcp_projection".into(), CValue::Str(projection.into()));
    v.insert("human_only".into(), CValue::Bool(human_only));
    CommandDef::from_value(CValue::Obj(v)).unwrap()
}

/// Defence in depth: the projection used on MCP is never looser than the class ceiling of
/// CONSTITUTION §3, whatever the definition declares (hand-labelled from the class table).
#[test]
fn the_class_ceiling_caps_a_declared_projection() {
    let expect = [
        ("R0", "run"),
        ("R1", "run"),
        ("W1", "run"),
        ("W2", "run"),
        ("I1", "run"),
        ("W3", "pending"),
        ("X1", "pending"),
        ("D1", "pending"),
        ("M1", "policy/not_on_mcp"),
        ("S1", "policy/not_on_mcp"),
        ("A0", "policy/not_on_mcp"),
    ];
    assert_eq!(expect.len(), SideEffect::ALL.len());
    for (class, want) in expect {
        let d = class_def(class, "exposed", false);
        let got = match authorise_projection(ChannelKind::Mcp, &d) {
            Ok(Projected::Run) => "run".to_string(),
            Ok(Projected::PendingRequest) => "pending".to_string(),
            Err(e) => format!("{}/{}", e.code(), e.reason()),
        };
        assert_eq!(got, want, "{class}");
        // human_only makes every class at most a pending request; never stays never.
        let h = class_def(class, "exposed", true);
        let got_h = authorise_projection(ChannelKind::ServerMcp, &h).map_err(|e| e.reason().to_string());
        let want_h =
            if want == "policy/not_on_mcp" { Err("not_on_mcp".to_string()) } else { Ok(Projected::PendingRequest) };
        assert_eq!(got_h, want_h, "{class} human_only");
        let n = class_def(class, "never", true);
        assert_eq!(mcp_projection(&n), Projection::Never);
        assert_err(authorise_projection(ChannelKind::Mcp, &n), Code::Policy, "not_on_mcp");
    }
}

/// The parameters commitments equal the reference's sha256 of the canonical JSON, and do not
/// depend on key order.
#[test]
fn params_commitments_match_the_reference() {
    let c = cases();
    for row in arr(obj(&c), "commitments") {
        let o = obj(row);
        let p = o.get("params").unwrap();
        assert_eq!(params_commitment(p).unwrap().as_str(), st(o, "commitment"));
    }
    let a = params_commitment(&json(r#"{"target":"plane-1","count":3}"#)).unwrap();
    let b = params_commitment(&json(r#"{"count":3,"target":"plane-1"}"#)).unwrap();
    assert_eq!(a, b);
    let filed = obj(obj(&c).get("filed").unwrap());
    assert_eq!(a.as_str(), st(filed, "commitment"));
}

// ------------------------------------------------------------------ AC-1, AC-2

struct Counting(Arc<AtomicUsize>);

impl Handler for Counting {
    fn run(
        &self,
        _view: &dyn StateView,
        _params: &ValidParams,
        _ctx: &HandlerCtx,
    ) -> Result<Vec<Event>, fid_bus::handler::HandlerError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

/// AC-1: an MCP agent invoking the request_only and the human_only command gets a request ID
/// each; the Record Journal gains one W1 `request.file_request@1` entry each, with the
/// command, parameters commitment, requester and head; no handler runs. The exposed command
/// runs its handler.
#[test]
fn ac1_mcp_calls_to_request_only_and_human_only_commands_file_requests() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let runs = Arc::new(AtomicUsize::new(0));
    let mut set = HandlerSet::new();
    for d in registry() {
        set.register(d, Box::new(Counting(Arc::clone(&runs)))).unwrap();
    }
    let p = filed_params();
    let mut ids = Vec::new();
    for (n, cmd) in [(1u8, REQUEST_CMD), (2u8, HUMAN_CMD)] {
        match w.authorise(&agent, cmd, &p, None).unwrap() {
            Decision::File(filing) => {
                assert_eq!(filing.command().as_str(), cmd);
                ids.push(w.commit_filing(&filing, rid(n)).unwrap());
            }
            Decision::Run | Decision::RunApproved(_) => {
                let ctx = HandlerCtx::derive(&w.table, &agent, 0, CancelToken::never()).unwrap();
                let view = w.book.view(HEAD);
                run_handler(&set, cmd, &view, &ValidParams::for_tests(p.clone()), &ctx).unwrap();
            }
        }
    }
    assert_eq!(ids, [rid(1), rid(2)]);
    assert_eq!(runs.load(Ordering::SeqCst), 0, "no handler ran");
    assert_eq!(w.journal.drafts.len(), 2);
    let commitment = params_commitment(&p).unwrap();
    for ((draft, id), cmd) in w.journal.drafts.iter().zip(&ids).zip([REQUEST_CMD, HUMAN_CMD]) {
        assert_eq!(draft.principal.kind, PrincipalKind::Agent);
        assert_eq!(draft.principal.identity, CValue::Str("claude-a".into()));
        assert_eq!(draft.ai_assistance, AiAssistance::Executed);
        assert_eq!((draft.stream.as_str(), draft.branch.as_str()), ("record", "main"));
        let DraftBody::Command(c) = &draft.body else { panic!() };
        assert_eq!(
            (c.id.as_str(), c.major, c.class, c.determinism),
            (FILE_REQUEST, 1, SideEffect::W1, Determinism::D0)
        );
        assert_eq!(c.inputs.len(), 1);
        assert_eq!(c.inputs[0].tag, PARAMS_TAG);
        assert_eq!(c.inputs[0].digest.to_string(), commitment.as_str());
        assert_eq!(
            canon(&c.args),
            canon(&json(&format!(r#"{{"command":"{cmd}","params":{{"count":3,"target":"plane-1"}}}}"#)))
        );
        let events = draft_events(draft);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].type_id().as_str(), REQUEST_FILED);
        let body = obj(events[0].body());
        assert_eq!(st(body, "request"), id.to_string());
        assert_eq!(st(body, "command"), cmd);
        assert_eq!(st(body, "params_commitment"), commitment.as_str());
        assert_eq!(st(body, "filed_at_head"), HEAD.to_string());
        assert_eq!(canon(body.get("requester").unwrap()), canon(&json(r#"{"id":"claude-a","kind":"agent"}"#)));
        assert_eq!(c.outputs.len(), 1);
        assert_eq!(c.outputs[0].entity.as_deref(), Some(id.to_string().as_str()));
        // The draft is a valid Record Journal entry.
        build(draft, 1, GENESIS_PREV, &mut FixedSalts).unwrap();
    }
    let recs: Vec<(u64, RequestState)> = ids.iter().map(|i| (w.record(*i).seq(), w.record(*i).state())).collect();
    assert_eq!(recs, [(1, RequestState::Pending), (2, RequestState::Pending)]);
    // The exposed command runs over MCP.
    assert_eq!(w.authorise(&agent, "test.run_exposed@1", &p, None).unwrap(), Decision::Run);
    let ctx = HandlerCtx::derive(&w.table, &agent, 0, CancelToken::never()).unwrap();
    let view = w.book.view(HEAD);
    run_handler(&set, "test.run_exposed@1", &view, &ValidParams::for_tests(p.clone()), &ctx).unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

/// AC-2: the `never` command (and a command with no MCP surface) over MCP is
/// `policy/not_on_mcp`, and nothing is journaled.
#[test]
fn ac2_never_over_mcp_is_refused_and_nothing_is_journaled() {
    let mut w = World::new();
    for kind in [ChannelKind::Mcp, ChannelKind::ServerMcp] {
        let agent = w.channel(kind, "claude-a");
        for cmd in ["test.run_never@1", "test.run_local@1"] {
            assert_err(w.authorise(&agent, cmd, &filed_params(), None), Code::Policy, "not_on_mcp");
        }
        // The internal filing command and the human-only reject are not on MCP either.
        let r = json(&format!(r#"{{"request":"{}","reason":"x"}}"#, rid(1)));
        assert_err(w.authorise(&agent, &format!("{REJECT_REQUEST}@1"), &r, None), Code::Policy, "not_on_mcp");
        let f = json(r#"{"command":"test.run_request@1","params":{}}"#);
        assert_err(w.authorise(&agent, &format!("{FILE_REQUEST}@1"), &f, None), Code::Policy, "not_on_mcp");
    }
    assert!(w.journal.drafts.is_empty());
    assert!(w.book.is_empty());
    // The same commands pass this check for a person in the GUI.
    let alice = w.channel(ChannelKind::Gui, "alice");
    assert_eq!(w.authorise(&alice, "test.run_never@1", &filed_params(), None).unwrap(), Decision::Run);
}

/// The principal comes from the channel only: fields in the parameters that name a principal
/// change nothing, and are part of the committed parameters.
#[test]
fn a_principal_named_in_the_parameters_is_ignored() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let p = json(r#"{"target":"plane-1","principal":{"kind":"human","id":"alice"},"approved_by":"alice"}"#);
    let id = w.file(&agent, REQUEST_CMD, &p, 1);
    let r = w.record(id);
    assert_eq!(r.requester(), &PrincipalRef { kind: PrincipalKind::Agent, id: pid("claude-a") });
    assert_eq!(r.params_commitment(), params_commitment(&p).unwrap());
    // A human_only command stays a pending request whatever the parameters say.
    let d = w.authorise(&agent, HUMAN_CMD, &p, None).unwrap();
    assert!(matches!(d, Decision::File(_)));
}

/// A request ID already in the book is `conflict/id_collision`, and nothing is appended.
#[test]
fn filing_under_an_existing_request_id_is_refused() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let Decision::File(f) = w.authorise(&agent, HUMAN_CMD, &filed_params(), None).unwrap() else { panic!() };
    let view = w.book.view(HEAD);
    let r = f.commit(rid(1), &view, time(), &record_ctx(), &mut w.journal);
    assert_err(r, Code::Conflict, "id_collision");
    assert_eq!(w.journal.drafts.len(), 1);
}

/// Request IDs are lowercase hyphenated UUIDv7 only.
#[test]
fn request_ids_are_uuid_v7() {
    assert!(RequestId::parse("0193a5b0-1c2d-7e3f-8a4b-000000000001").is_ok());
    for bad in [
        "0193A5B0-1C2D-7E3F-8A4B-000000000001",
        "0193a5b0-1c2d-4e3f-8a4b-000000000001",
        "0193a5b0-1c2d-7e3f-ca4b-000000000001",
        "00000000-0000-0000-0000-000000000000",
        "0193a5b01c2d7e3f8a4b000000000001",
        "",
    ] {
        assert_err(RequestId::parse(bad), Code::Validation, "schema");
    }
}

// ------------------------------------------------------------------ AC-3, AC-4, REQ-008

fn approve_entry(w: &mut World, approval: &Approval) {
    let events =
        [Event::new(fid_bus::handler::EventType::new("TestDone").unwrap(), json("{}")), approval.event().unwrap()];
    w.book.apply_entry(&events).unwrap();
}

/// AC-3: a GUI human invoking the same command and parameters (keys in another order) with
/// `approval_of: R` is allowed as `human`; the commit carries `RequestApproved` naming the
/// agent as requester and R; folded, R is `approved` in the list.
#[test]
fn ac3_a_human_approval_runs_as_human_and_records_the_requester() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = json(r#"{"count":3,"target":"plane-1"}"#);
    let view = w.book.view(HEAD);
    let a = authorise(&w.table, &alice, &def(REQUEST_CMD), &p, Some(&r), &view).unwrap();
    assert_eq!(a.principal().kind(), PrincipalKind::Human);
    let Decision::RunApproved(approval) = a.decision().clone() else { panic!("{:?}", a.decision()) };
    assert_eq!(approval.request(), r);
    assert_eq!(approval.requested_by(), &PrincipalRef { kind: PrincipalKind::Agent, id: pid("claude-a") });
    assert_eq!(approval.approved_by(), &PrincipalRef { kind: PrincipalKind::Human, id: pid("alice") });
    let e = approval.event().unwrap();
    assert_eq!(e.type_id().as_str(), REQUEST_APPROVED);
    let body = obj(e.body());
    assert_eq!(st(body, "request"), r.to_string());
    assert_eq!(st(body, "command"), REQUEST_CMD);
    assert_eq!(st(body, "record_digest"), w.record(r).record_digest().unwrap().as_str());
    assert_eq!(canon(body.get("requested_by").unwrap()), canon(&json(r#"{"id":"claude-a","kind":"agent"}"#)));
    assert_eq!(canon(body.get("approved_by").unwrap()), canon(&json(r#"{"id":"alice","kind":"human"}"#)));
    approve_entry(&mut w, &approval);
    let listed = w.list(&alice, &json("{}")).unwrap();
    let reqs = arr(obj(&listed), "requests");
    assert_eq!(reqs.len(), 1);
    let one = obj(&reqs[0]);
    assert_eq!(st(one, "request"), r.to_string());
    assert_eq!(st(one, "state"), "approved");
    assert_eq!(canon(one.get("resolved_by").unwrap()), canon(&json(r#"{"id":"alice","kind":"human"}"#)));
    // Replay of the same approval is refused: R is no longer pending.
    assert_err(w.authorise(&alice, REQUEST_CMD, &p, Some(&r)), Code::Policy, "bad_approval");
    assert!(w.book.apply_entry(&[approval.event().unwrap()]).is_err());
    assert_eq!(w.record(r).state(), RequestState::Approved);
}

/// AC-4 and the approval refusals of the fixture: other parameters, other command, any
/// non-human approver; plus unknown, withdrawn and rejected requests.
#[test]
fn ac4_approval_refusals() {
    let c = cases();
    for row in arr(obj(&c), "approvals") {
        let o = obj(row);
        let mut w = World::new();
        let agent = w.channel(ChannelKind::Mcp, "claude-a");
        let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
        let who = w.channel(kind_of(st(o, "approver")), "alice");
        let got = w.authorise(&who, st(o, "command"), o.get("params").unwrap(), Some(&r));
        let name = st(o, "name");
        match st(o, "expect") {
            "approve" => assert!(matches!(got, Ok(Decision::RunApproved(_))), "{name}: {got:?}"),
            expect => {
                let e = got.unwrap_err();
                assert_eq!(format!("{}/{}", e.code(), e.reason()), expect, "{name}");
            }
        }
        // A refusal changes nothing.
        assert_eq!(w.record(r).state(), RequestState::Pending, "{name}");
        assert_eq!(w.journal.drafts.len(), 1, "{name}");
    }

    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let alice = w.channel(ChannelKind::Gui, "alice");
    assert_err(w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&rid(9))), Code::NotFound, "unknown_request");
    // Withdrawn and rejected requests cannot be approved.
    let r1 = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let ev = w.run(&agent, &format!("{WITHDRAW_REQUEST}@1"), &json(&format!(r#"{{"request":"{r1}"}}"#))).unwrap();
    w.book.apply_entry(&ev).unwrap();
    assert_err(w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r1)), Code::Policy, "bad_approval");
    let r2 = w.file(&agent, REQUEST_CMD, &filed_params(), 2);
    let ev = w
        .run(&alice, &format!("{REJECT_REQUEST}@1"), &json(&format!(r#"{{"request":"{r2}","reason":"no"}}"#)))
        .unwrap();
    w.book.apply_entry(&ev).unwrap();
    assert_err(w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r2)), Code::Policy, "bad_approval");
}

/// REQ-008: an approval that passed the check but whose later stage failed leaves the request
/// pending; nothing changes until the approving entry is committed and folded.
#[test]
fn a_failed_approval_leaves_the_request_pending() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let alice = w.channel(ChannelKind::Gui, "alice");
    let first = w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r)).unwrap();
    assert!(matches!(first, Decision::RunApproved(_)));
    drop(first); // a later pipeline stage failed: nothing is committed
    assert_eq!(w.record(r).state(), RequestState::Pending);
    assert_eq!(w.journal.drafts.len(), 1);
    let Decision::RunApproved(again) = w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r)).unwrap() else {
        panic!()
    };
    approve_entry(&mut w, &again);
    assert_eq!(w.record(r).state(), RequestState::Approved);
}

/// The fold accepts only an approval bound to the exact record, by a human, once.
#[test]
fn the_fold_refuses_a_substituted_or_forged_approval() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let alice = w.channel(ChannelKind::Gui, "alice");
    let Decision::RunApproved(a) = w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r)).unwrap() else {
        panic!()
    };
    let good = a.event().unwrap();
    let mutate = |k: &str, v: CValue| {
        let mut b = obj(good.body()).clone();
        b.insert(k.into(), v);
        Event::new(good.type_id().clone(), CValue::Obj(b))
    };
    let other = "sha256:0000000000000000000000000000000000000000000000000000000000000001";
    for (name, e) in [
        ("other record digest", mutate("record_digest", CValue::Str(other.into()))),
        ("other command", mutate("command", CValue::Str(HUMAN_CMD.into()))),
        ("other requester", mutate("requested_by", json(r#"{"id":"claude-b","kind":"agent"}"#))),
        ("agent approver", mutate("approved_by", json(r#"{"id":"claude-a","kind":"agent"}"#))),
        ("script approver", mutate("approved_by", json(r#"{"id":"alice","kind":"script"}"#))),
        ("unknown request", mutate("request", CValue::Str(rid(9).to_string()))),
        ("extra field", mutate("note", CValue::Null)),
    ] {
        assert_err(w.book.apply_entry(std::slice::from_ref(&e)), Code::Internal, "invariant_violated");
        assert_eq!(w.record(r).state(), RequestState::Pending, "{name}");
    }
    // Two events for one request in one entry are refused, and neither is applied.
    let wd = Event::new(
        fid_bus::handler::EventType::new(REQUEST_WITHDRAWN).unwrap(),
        json(&format!(r#"{{"request":"{r}","withdrawn_by":{{"id":"claude-a","kind":"agent"}}}}"#)),
    );
    assert_err(w.book.apply_entry(&[good.clone(), wd]), Code::Internal, "invariant_violated");
    assert_eq!(w.record(r).state(), RequestState::Pending);
    w.book.apply_entry(&[good]).unwrap();
    assert_eq!(w.record(r).state(), RequestState::Approved);
}

// ------------------------------------------------------------------ AC-5, AC-6, list

/// AC-5: the requester withdraws R; another agent, or the same name as another kind, gets
/// `policy/not_requester`; a second withdrawal is `precondition/invalid_transition`.
#[test]
fn ac5_only_the_requester_withdraws() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let cmd = format!("{WITHDRAW_REQUEST}@1");
    let p = json(&format!(r#"{{"request":"{r}"}}"#));
    let other = w.channel(ChannelKind::Mcp, "claude-b");
    assert_err(w.run(&other, &cmd, &p), Code::Policy, "not_requester");
    let same_name_script = w.channel(ChannelKind::Cli, "claude-a");
    assert_err(w.run(&same_name_script, &cmd, &p), Code::Policy, "not_requester");
    let alice = w.channel(ChannelKind::Gui, "alice");
    assert_err(w.run(&alice, &cmd, &p), Code::Policy, "not_requester");
    // The withdraw command is exposed on MCP.
    assert_eq!(w.authorise(&agent, &cmd, &p, None).unwrap(), Decision::Run);
    let events = w.run(&agent, &cmd, &p).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].type_id().as_str(), REQUEST_WITHDRAWN);
    w.book.apply_entry(&events).unwrap();
    assert_eq!(w.record(r).state(), RequestState::Withdrawn);
    assert_eq!(w.record(r).resolved_by(), Some(&PrincipalRef { kind: PrincipalKind::Agent, id: pid("claude-a") }));
    assert_err(w.run(&agent, &cmd, &p), Code::Precondition, "invalid_transition");
    assert_err(
        w.run(&agent, &cmd, &json(&format!(r#"{{"request":"{}"}}"#, rid(9)))),
        Code::NotFound,
        "unknown_request",
    );
    for bad in [
        format!(r#"{{"request":"{r}","requester":{{"id":"claude-a","kind":"agent"}}}}"#),
        format!(r#"{{"request":"{}"}}"#, r.to_string().to_uppercase()),
        r#"{"request":7}"#.to_string(),
        "{}".to_string(),
    ] {
        assert_err(w.run(&agent, &cmd, &json(&bad)), Code::Validation, "schema");
    }
}

/// AC-6: a human rejects R with a reason; R is `rejected` and the reason is kept as untrusted
/// text with its commitment (the reference's digest). Non-humans cannot reject.
#[test]
fn ac6_a_human_rejects_with_an_untrusted_reason() {
    let c = cases();
    let rej = obj(obj(&c).get("reject").unwrap());
    let reason = st(rej, "reason");
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let cmd = format!("{REJECT_REQUEST}@1");
    let p = json(&format!(r#"{{"request":"{r}","reason":"{reason}"}}"#));
    // Agents and scripts cannot reject: the class check and the handler both refuse.
    let reject_def = def(&cmd);
    for (kind, name) in [(ChannelKind::Mcp, "claude-a"), (ChannelKind::Cli, "bob"), (ChannelKind::Service, "svc")] {
        let ch = w.channel(kind, name);
        assert_err(w.run(&ch, &cmd, &p), Code::Policy, "human_only");
        let principal = w.table.derive_principal(&ch).unwrap();
        assert_err(authorise_class(&principal, &reject_def), Code::Policy, "human_only");
    }
    let alice = w.channel(ChannelKind::Gui, "alice");
    let events = w.run(&alice, &cmd, &p).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].type_id().as_str(), REQUEST_REJECTED);
    let body = obj(events[0].body());
    assert_eq!(st(body, "reason"), reason);
    assert_eq!(st(body, "reason_commitment"), st(rej, "commitment"));
    w.book.apply_entry(&events).unwrap();
    let rec = w.record(r);
    assert_eq!(rec.state(), RequestState::Rejected);
    assert_eq!(rec.reason().map(|u| u.as_untrusted().as_str()), Some(reason));
    let listed = w.list(&alice, &json(r#"{"state":"rejected"}"#)).unwrap();
    let one = obj(&arr(obj(&listed), "requests")[0]);
    assert_eq!(st(one, "reason"), reason);
    assert_err(w.run(&alice, &cmd, &p), Code::Precondition, "invalid_transition");
    assert_err(
        w.run(&alice, &cmd, &json(&format!(r#"{{"request":"{}","reason":"x"}}"#, rid(9)))),
        Code::NotFound,
        "unknown_request",
    );
    // Reason length: 1 to 2000 characters.
    let r2 = w.file(&agent, REQUEST_CMD, &filed_params(), 2);
    let with = |reason: &str| {
        let mut o = Obj::new();
        o.insert("request".into(), CValue::Str(r2.to_string()));
        o.insert("reason".into(), CValue::Str(reason.into()));
        CValue::Obj(o)
    };
    assert_err(w.run(&alice, &cmd, &with("")), Code::Validation, "schema");
    assert_err(w.run(&alice, &cmd, &with(&"\u{e9}".repeat(MAX_REASON_CHARS + 1))), Code::Validation, "schema");
    assert_err(w.run(&alice, &cmd, &json(&format!(r#"{{"request":"{r2}"}}"#))), Code::Validation, "schema");
    let ok = w.run(&alice, &cmd, &with(&"\u{e9}".repeat(MAX_REASON_CHARS))).unwrap();
    w.book.apply_entry(&ok).unwrap();
    assert_eq!(w.record(r2).state(), RequestState::Rejected);
    // A forged rejection whose reason does not match its commitment is refused by the fold.
    let r3 = w.file(&agent, REQUEST_CMD, &filed_params(), 3);
    let mut b = obj(events[0].body()).clone();
    b.insert("request".into(), CValue::Str(r3.to_string()));
    b.insert("reason".into(), CValue::Str("other".into()));
    let forged = Event::new(events[0].type_id().clone(), CValue::Obj(b));
    assert_err(w.book.apply_entry(&[forged]), Code::Internal, "invariant_violated");
    assert_eq!(w.record(r3).state(), RequestState::Pending);
}

/// REQ-007: an agent sees its own requests only, a human sees all, by filing sequence, and
/// the state filter applies.
#[test]
fn list_requests_shows_own_or_all_by_filing_sequence() {
    let mut w = World::new();
    let a = w.channel(ChannelKind::Mcp, "claude-a");
    let b = w.channel(ChannelKind::ServerMcp, "claude-b");
    let alice = w.channel(ChannelKind::Gui, "alice");
    // IDs out of filing order, so sorting by ID would differ.
    let r1 = w.file(&a, REQUEST_CMD, &filed_params(), 9);
    let r2 = w.file(&b, HUMAN_CMD, &filed_params(), 3);
    let r3 = w.file(&a, HUMAN_CMD, &json(r#"{"target":"x"}"#), 1);
    let ev = w.run(&a, &format!("{WITHDRAW_REQUEST}@1"), &json(&format!(r#"{{"request":"{r3}"}}"#))).unwrap();
    w.book.apply_entry(&ev).unwrap();
    let ids = |v: CValue| -> Vec<String> {
        arr(obj(&v), "requests").iter().map(|x| st(obj(x), "request").to_string()).collect()
    };
    let s = |r: RequestId| r.to_string();
    assert_eq!(ids(w.list(&a, &json("{}")).unwrap()), [s(r1), s(r3)]);
    assert_eq!(ids(w.list(&b, &json("{}")).unwrap()), [s(r2)]);
    assert_eq!(ids(w.list(&alice, &json("{}")).unwrap()), [s(r1), s(r2), s(r3)]);
    assert_eq!(ids(w.list(&alice, &json(r#"{"state":"pending"}"#)).unwrap()), [s(r1), s(r2)]);
    assert_eq!(ids(w.list(&a, &json(r#"{"state":"withdrawn"}"#)).unwrap()), [s(r3)]);
    assert!(ids(w.list(&b, &json(r#"{"state":"approved"}"#)).unwrap()).is_empty());
    let seqs: Vec<u64> = [r1, r2, r3].iter().map(|r| w.record(*r).seq()).collect();
    assert_eq!(seqs, [1, 2, 3]);
    // A script on the CLI with the same name as the agent sees nothing of the agent's.
    let script = w.channel(ChannelKind::Cli, "claude-a");
    assert!(ids(w.list(&script, &json("{}")).unwrap()).is_empty());
    for bad in [r#"{"state":"open"}"#, r#"{"state":1}"#, r#"{"principal":"human"}"#, "[]"] {
        assert_err(w.list(&alice, &json(bad)), Code::Validation, "schema");
    }
    // The list command is exposed on MCP.
    assert_eq!(w.authorise(&a, &format!("{LIST_REQUESTS}@1"), &json("{}"), None).unwrap(), Decision::Run);
}

/// Records round-trip through their entity body, and the book exposes them as entities of
/// kind `request.pending_request`.
#[test]
fn records_round_trip_through_the_state_view() {
    let mut w = World::new();
    let a = w.channel(ChannelKind::Mcp, "claude-a");
    let r = w.file(&a, REQUEST_CMD, &filed_params(), 1);
    let view = w.book.view(HEAD);
    assert_eq!(view.head(), HEAD);
    let e = view.entity(&fid_bus::handler::EntityId::new(&r.to_string()).unwrap()).unwrap();
    assert_eq!(e.kind, REQUEST_KIND);
    assert_eq!(RequestRecord::from_value(e.body).unwrap(), w.record(r));
    assert_eq!(RequestRecord::read(&view, &r).unwrap(), Some(w.record(r)));
    assert_eq!(RequestRecord::read(&view, &rid(2)).unwrap(), None);
}

// ------------------------------------------------------------------ AC-7 (in part)

/// The filing and approval events are a pure function of their recorded inputs (request ID,
/// principals, parameters, head), and folding the same journal twice gives the same book. The
/// exact-mode replay of AC-7 itself needs CORE.RPL.EXEC.LOOP.
#[test]
fn filing_and_folding_are_deterministic() {
    let run_once = || {
        let mut w = World::new();
        let a = w.channel(ChannelKind::Mcp, "claude-a");
        let r = w.file(&a, REQUEST_CMD, &filed_params(), 1);
        let alice = w.channel(ChannelKind::Gui, "alice");
        let Decision::RunApproved(ap) = w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r)).unwrap() else {
            panic!()
        };
        approve_entry(&mut w, &ap);
        let entries: Vec<Vec<u8>> = w
            .journal
            .drafts
            .iter()
            .map(|d| canon(&build(d, 1, GENESIS_PREV, &mut FixedSalts).unwrap().header.to_value()))
            .collect();
        let listed = canon(&w.list(&alice, &json("{}")).unwrap());
        (entries, canon(&ap.event().unwrap().to_value()), listed, w.journal.drafts)
    };
    let (e1, a1, l1, drafts) = run_once();
    let (e2, a2, l2, _) = run_once();
    assert_eq!((e1, a1, l1), (e2, a2, l2));
    // Re-folding the recorded journal gives the same records.
    let mut book = RequestBook::new();
    for d in &drafts {
        book.apply_entry(&draft_events(d)).unwrap();
    }
    assert_eq!(book.len(), 1);
    assert_eq!(book.records()[0].state(), RequestState::Pending);
}

// ------------------------------------------------------------------ command definitions

/// ID, class, projection, `human_only` and surfaces of one definition.
type DefRow = (String, &'static str, &'static str, bool, Vec<&'static str>);

/// The four request commands as the spec's Design declares them.
#[test]
fn command_definitions_match_the_design() {
    let defs = command_defs().unwrap();
    let got: Vec<DefRow> = defs
        .iter()
        .map(|d| {
            (
                format!("{}@{}", d.id, d.major),
                d.side_effect.as_str(),
                d.mcp_projection.as_str(),
                d.human_only,
                d.exposure.iter().map(|s| s.as_str()).collect(),
            )
        })
        .collect();
    let want: Vec<DefRow> = vec![
        ("request.list_requests@1".into(), "R0", "exposed", false, vec!["cli", "mcp"]),
        ("request.withdraw_request@1".into(), "W1", "exposed", false, vec!["cli", "mcp"]),
        ("request.reject_request@1".into(), "W1", "never", true, vec!["cli", "gui"]),
        ("request.file_request@1".into(), "W1", "never", false, vec![]),
    ];
    assert_eq!(got, want);
    let effects: Vec<Vec<String>> = defs.iter().map(|d| d.effects.clone()).collect();
    assert_eq!(
        effects,
        [vec![], vec!["RequestWithdrawn".to_string()], vec!["RequestRejected".into()], vec!["RequestFiled".into()]]
    );
    assert!(defs.iter().all(|d| d.determinism == Determinism::D0));
    let catalogue = Catalogue::geo_frame().unwrap();
    for d in &defs {
        assert_eq!(check_schema(&d.params_schema, &catalogue), vec![], "{}", d.id);
        assert_eq!(check_schema(&d.result_schema, &catalogue), vec![], "{}", d.id);
    }
    let decls: Vec<CommandDecl> = defs.iter().map(CommandDecl::from_def).collect();
    assert_eq!(fid_registry::validate(&decls), vec![]);
}

/// A checker for the JSON Schema subset the request definitions use. An unknown keyword or
/// pattern fails the test, so a schema change must extend this checker on purpose.
fn conforms(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let CValue::Obj(s) = schema else { return Err(format!("{at}: schema is not an object")) };
    for (k, kv) in s {
        let ok = match (k.as_str(), kv) {
            ("description", _) | ("x-untrusted", CValue::Bool(true)) => true,
            ("type", CValue::Str(t)) => match (t.as_str(), v) {
                ("object", CValue::Obj(_)) | ("string", CValue::Str(_)) | ("array", CValue::Arr(_)) => true,
                ("integer", CValue::Num(n)) => n.fract() == 0.0,
                _ => false,
            },
            ("enum", CValue::Arr(options)) => options.contains(v),
            ("minimum", CValue::Num(m)) => matches!(v, CValue::Num(n) if n >= m),
            ("minLength", CValue::Num(m)) => matches!(v, CValue::Str(x) if x.chars().count() as f64 >= *m),
            ("maxLength", CValue::Num(m)) => matches!(v, CValue::Str(x) if x.chars().count() as f64 <= *m),
            ("pattern", CValue::Str(p)) => {
                let CValue::Str(x) = v else { return Err(format!("{at}: pattern on a non-string")) };
                matches_pattern(p, x)
            }
            ("required", CValue::Arr(r)) => {
                let CValue::Obj(o) = v else { return Err(format!("{at}: required on a non-object")) };
                r.iter().all(|k| matches!(k, CValue::Str(k) if o.contains_key(k)))
            }
            ("additionalProperties", CValue::Bool(false)) => {
                let (CValue::Obj(o), Some(CValue::Obj(props))) = (v, s.get("properties")) else {
                    return Err(format!("{at}: additionalProperties without properties or object"));
                };
                o.keys().all(|k| props.contains_key(k))
            }
            ("properties", CValue::Obj(props)) => {
                if let CValue::Obj(o) = v {
                    for (pk, ps) in props {
                        if let Some(pv) = o.get(pk) {
                            conforms(ps, pv, &format!("{at}/{pk}"))?;
                        }
                    }
                }
                true
            }
            ("items", item) => {
                if let CValue::Arr(xs) = v {
                    for (i, x) in xs.iter().enumerate() {
                        conforms(item, x, &format!("{at}/{i}"))?;
                    }
                }
                true
            }
            (other, _) => return Err(format!("{at}: keyword {other} not supported by the test checker")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}

fn all_hex(b: &[u8]) -> bool {
    b.iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// The three patterns the definitions use, matched by hand (no regex engine in the workspace).
fn matches_pattern(p: &str, x: &str) -> bool {
    let b = x.as_bytes();
    match p {
        "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$" => {
            b.len() == 36
                && b.iter().enumerate().all(|(i, c)| match i {
                    8 | 13 | 18 | 23 => *c == b'-',
                    14 => *c == b'7',
                    19 => matches!(c, b'8' | b'9' | b'a' | b'b'),
                    _ => all_hex(&[*c]),
                })
        }
        "^sha256:[0-9a-f]{64}$" => b.len() == 71 && x.starts_with("sha256:") && all_hex(&b[7..]),
        "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@[0-9]{1,5}$" => match x.rsplit_once('@') {
            Some((name, major)) => {
                let edge = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
                let nb = name.as_bytes();
                !nb.is_empty()
                    && edge(nb[0])
                    && edge(nb[nb.len() - 1])
                    && nb.iter().all(|c| edge(*c) || matches!(c, b'_' | b'.'))
                    && (1..=5).contains(&major.len())
                    && major.bytes().all(|c| c.is_ascii_digit())
            }
            None => false,
        },
        other => panic!("pattern {other} not supported by the test checker"),
    }
}

/// Real parameters of every request command conform to its `params_schema`; malformed ones do
/// not.
#[test]
fn params_conform_to_the_params_schemas() {
    let defs = command_defs().unwrap();
    let schema = |id: &str| defs.iter().find(|d| d.id == id).unwrap().params_schema.clone();
    let r = rid(1);
    let ok = [
        (LIST_REQUESTS, "{}".to_string()),
        (LIST_REQUESTS, r#"{"state":"pending"}"#.to_string()),
        (WITHDRAW_REQUEST, format!(r#"{{"request":"{r}"}}"#)),
        (REJECT_REQUEST, format!(r#"{{"request":"{r}","reason":"wrong datum"}}"#)),
        (FILE_REQUEST, r#"{"command":"test.run_request@1","params":{"count":3,"target":"plane-1"}}"#.to_string()),
    ];
    for (id, p) in &ok {
        conforms(&schema(id), &json(p), id).unwrap();
    }
    let long = "x".repeat(MAX_REASON_CHARS + 1);
    let bad = [
        (LIST_REQUESTS, r#"{"state":"open"}"#.to_string()),
        (LIST_REQUESTS, r#"{"principal":"human"}"#.to_string()),
        (WITHDRAW_REQUEST, "{}".to_string()),
        (WITHDRAW_REQUEST, format!(r#"{{"request":"{}"}}"#, r.to_string().to_uppercase())),
        (WITHDRAW_REQUEST, r#"{"request":"0193a5b0-1c2d-4e3f-8a4b-000000000001"}"#.to_string()),
        (WITHDRAW_REQUEST, format!(r#"{{"request":"{r}","requester":"x"}}"#)),
        (REJECT_REQUEST, format!(r#"{{"request":"{r}","reason":""}}"#)),
        (REJECT_REQUEST, format!(r#"{{"request":"{r}","reason":"{long}"}}"#)),
        (REJECT_REQUEST, format!(r#"{{"request":"{r}"}}"#)),
        (FILE_REQUEST, r#"{"command":"Test@1","params":{}}"#.to_string()),
    ];
    for (id, p) in &bad {
        assert!(conforms(&schema(id), &json(p), id).is_err(), "{id} accepts {p}");
    }
}

/// Real results conform to each command's `result_schema`: the list result of a book with
/// pending, approved, withdrawn and rejected requests, the empty results of withdraw and
/// reject, and the filing result; malformed results do not.
#[test]
fn results_conform_to_the_result_schemas() {
    let defs = command_defs().unwrap();
    let schema = |id: &str| defs.iter().find(|d| d.id == id).unwrap().result_schema.clone();
    let mut w = World::new();
    let a = w.channel(ChannelKind::Mcp, "claude-a");
    let alice = w.channel(ChannelKind::Gui, "alice");
    let r1 = w.file(&a, REQUEST_CMD, &filed_params(), 1);
    let r2 = w.file(&a, REQUEST_CMD, &json(r#"{"target":"y"}"#), 2);
    let r3 = w.file(&a, HUMAN_CMD, &filed_params(), 3);
    w.file(&a, HUMAN_CMD, &json(r#"{"target":"z"}"#), 4);
    let Decision::RunApproved(ap) = w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r1)).unwrap() else {
        panic!()
    };
    approve_entry(&mut w, &ap);
    let ev = w.run(&a, &format!("{WITHDRAW_REQUEST}@1"), &json(&format!(r#"{{"request":"{r2}"}}"#))).unwrap();
    w.book.apply_entry(&ev).unwrap();
    let ev = w
        .run(&alice, &format!("{REJECT_REQUEST}@1"), &json(&format!(r#"{{"request":"{r3}","reason":"wrong datum"}}"#)))
        .unwrap();
    w.book.apply_entry(&ev).unwrap();
    let listed = w.list(&alice, &json("{}")).unwrap();
    let states: Vec<&str> = arr(obj(&listed), "requests").iter().map(|x| st(obj(x), "state")).collect();
    assert_eq!(states, ["approved", "withdrawn", "rejected", "pending"]);
    conforms(&schema(LIST_REQUESTS), &listed, "list").unwrap();
    conforms(&schema(LIST_REQUESTS), &json(r#"{"requests":[]}"#), "empty list").unwrap();
    conforms(&schema(WITHDRAW_REQUEST), &json("{}"), "withdraw").unwrap();
    conforms(&schema(REJECT_REQUEST), &json("{}"), "reject").unwrap();
    conforms(&schema(FILE_REQUEST), &json(&format!(r#"{{"request":"{r1}"}}"#)), "file").unwrap();
    let mut extra = obj(&arr(obj(&listed), "requests")[0]).clone();
    extra.insert("principal".into(), CValue::Str("human".into()));
    let one = |x: CValue| CValue::Obj([("requests".to_string(), CValue::Arr(vec![x]))].into_iter().collect());
    let mut bad_state = obj(&arr(obj(&listed), "requests")[0]).clone();
    bad_state.insert("state".into(), CValue::Str("open".into()));
    let mut no_seq = obj(&arr(obj(&listed), "requests")[0]).clone();
    no_seq.remove("seq");
    for (name, bad) in [
        ("extra record field", one(CValue::Obj(extra))),
        ("bad state", one(CValue::Obj(bad_state))),
        ("missing seq", one(CValue::Obj(no_seq))),
        ("no requests", json("{}")),
    ] {
        assert!(conforms(&schema(LIST_REQUESTS), &bad, name).is_err(), "{name}");
    }
    assert!(conforms(&schema(WITHDRAW_REQUEST), &json(r#"{"x":1}"#), "withdraw").is_err());
    assert!(conforms(&schema(FILE_REQUEST), &json(r#"{"request":"r"}"#), "file").is_err());
}
