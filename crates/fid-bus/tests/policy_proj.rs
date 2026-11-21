//! Acceptance criteria of spec node CORE.BUS.POL.PROJ (AC-1 .. AC-6; AC-7 in part), against
//! `fixtures/core/bus/pol/proj/registry.json` and `cases.json`, written by the independent
//! reference `fixtures/core/bus/pol/proj/make_cases.py` (outcomes hand-labelled from the spec,
//! commitments computed with Python's hashlib).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code};
use fid_bus::handler::{run_handler, CancelToken, Event, Handler, HandlerCtx, HandlerSet, StateView, ValidParams};
use fid_bus::policy::class::authorise_class;
use fid_bus::policy::projection::{
    authorise, authorise_projection, command_defs, forbidden_in_reason, is_mcp_channel, list_requests, mcp_projection,
    params_commitment, Approval, Decision, EntryOrigin, Filing, Projected, RecordContext, RejectHandler, RequestBook,
    RequestId, RequestRecord, RequestState, WithdrawHandler, FILE_REQUEST, LIST_REQUESTS, MAX_FILED_PARAMS_BYTES,
    MAX_REASON_CHARS, PARAMS_TAG, REASON_FORBIDDEN_RANGES, REJECT_REQUEST, REQUEST_APPROVED, REQUEST_FILED,
    REQUEST_KIND, REQUEST_REJECTED, REQUEST_WITHDRAWN, WITHDRAW_REQUEST,
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
use fid_types::{CommandId, Determinism, SideEffect};
use std::cell::RefCell;
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
    last: RefCell<Option<EntryOrigin>>,
}

impl World {
    fn new() -> World {
        World {
            table: SessionTable::new(),
            book: RequestBook::new(),
            journal: Journal::default(),
            last: RefCell::new(None),
        }
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
        authorise(&self.table, ch, &def(id), &ValidParams::for_tests(p.clone()), approval_of, &view)
            .map(|a| a.into_parts().1)
    }

    /// Files `p` for command `id` over `ch` under request ID `n` and folds the entry.
    fn file(&mut self, ch: &TestChannel, id: &str, p: &CValue, n: u8) -> RequestId {
        let Decision::File(filing) = self.authorise(ch, id, p, None).unwrap() else { panic!("{id} did not file") };
        self.commit_filing(&filing, rid(n)).unwrap()
    }

    fn commit_filing(&mut self, filing: &Filing, id: RequestId) -> Result<RequestId, BusError> {
        let view = self.book.view(HEAD);
        let got = filing.commit(id, &view, time(), &record_ctx(), &mut self.journal)?;
        let draft = self.journal.drafts.last().unwrap();
        let events = draft_events(draft);
        self.book.apply_entry(&EntryOrigin::from_draft(draft).unwrap(), &events).unwrap();
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
        let checked = run_handler(&set, id, &view, &ValidParams::for_tests(p.clone()), &ctx)?;
        let origin =
            EntryOrigin::new(ctx.principal().reference(), CommandId::parse(id).unwrap(), params_commitment(p).unwrap());
        *self.last.borrow_mut() = Some(origin);
        Ok(checked.events().to_vec())
    }

    /// The origin of an entry committed by `ch` for command `id` with the fixture's filed
    /// parameters.
    fn origin(&self, ch: &TestChannel, id: &str) -> EntryOrigin {
        let p = params_commitment(&filed_params()).unwrap();
        EntryOrigin::new(self.table.derive_principal(ch).unwrap().reference(), CommandId::parse(id).unwrap(), p)
    }

    /// Folds `events` as the entry of the last successful [`World::run`].
    fn fold_last_result(&mut self, events: &[Event]) -> Result<(), BusError> {
        let origin = self.last.borrow().clone().unwrap();
        self.book.apply_entry(&origin, events)
    }

    fn fold_last(&mut self, events: &[Event]) {
        self.fold_last_result(events).unwrap();
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
        ("M1", "not_found/unknown_command"),
        ("S1", "not_found/unknown_command"),
        ("A0", "not_found/unknown_command"),
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
        let want_h = if want == "not_found/unknown_command" {
            Err("unknown_command".to_string())
        } else {
            Ok(Projected::PendingRequest)
        };
        assert_eq!(got_h, want_h, "{class} human_only");
        let n = class_def(class, "never", true);
        assert_eq!(mcp_projection(&n), Projection::Never);
        assert_err(authorise_projection(ChannelKind::Mcp, &n), Code::NotFound, "unknown_command");
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
/// `not_found/unknown_command`, exactly as an unknown command, and nothing is journaled.
#[test]
fn ac2_never_over_mcp_is_refused_and_nothing_is_journaled() {
    let mut w = World::new();
    for kind in [ChannelKind::Mcp, ChannelKind::ServerMcp] {
        let agent = w.channel(kind, "claude-a");
        for cmd in ["test.run_never@1", "test.run_local@1"] {
            assert_err(w.authorise(&agent, cmd, &filed_params(), None), Code::NotFound, "unknown_command");
        }
        // The internal filing command and the human-only reject are not on MCP either.
        let r = json(&format!(r#"{{"request":"{}","reason":"x"}}"#, rid(1)));
        assert_err(w.authorise(&agent, &format!("{REJECT_REQUEST}@1"), &r, None), Code::NotFound, "unknown_command");
        let f = json(r#"{"command":"test.run_request@1","params":{}}"#);
        assert_err(w.authorise(&agent, &format!("{FILE_REQUEST}@1"), &f, None), Code::NotFound, "unknown_command");
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
    // Fields that name a principal are not part of the target's params_schema: refused.
    let p = json(r#"{"target":"plane-1","principal":{"kind":"human","id":"alice"},"approved_by":"alice"}"#);
    assert_err(w.authorise(&agent, REQUEST_CMD, &p, None), Code::Validation, "schema");
    assert_err(w.authorise(&agent, HUMAN_CMD, &p, None), Code::Validation, "schema");
    assert!(w.journal.drafts.is_empty());
    // The requester is the channel's principal.
    let id = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let r = w.record(id);
    assert_eq!(r.requester(), &PrincipalRef { kind: PrincipalKind::Agent, id: pid("claude-a") });
    assert_eq!(r.params_commitment(), params_commitment(&filed_params()).unwrap());
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

/// The origin of the approved command's entry: the approving human, the approved command.
fn approval_origin(approval: &Approval) -> EntryOrigin {
    EntryOrigin::new(
        approval.approved_by().clone(),
        CommandId::parse(REQUEST_CMD).unwrap(),
        approval.params_commitment(),
    )
}

fn approve_entry(w: &mut World, approval: &Approval) {
    let events =
        [Event::new(fid_bus::handler::EventType::new("TestDone").unwrap(), json("{}")), approval.event().unwrap()];
    w.book.apply_entry(&approval_origin(approval), &events).unwrap();
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
    let a =
        authorise(&w.table, &alice, &def(REQUEST_CMD), &ValidParams::for_tests(p.clone()), Some(&r), &view).unwrap();
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
    assert!(w.book.apply_entry(&approval_origin(&approval), &[approval.event().unwrap()]).is_err());
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
    w.fold_last(&ev);
    assert_err(w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r1)), Code::Policy, "bad_approval");
    let r2 = w.file(&agent, REQUEST_CMD, &filed_params(), 2);
    let ev = w
        .run(&alice, &format!("{REJECT_REQUEST}@1"), &json(&format!(r#"{{"request":"{r2}","reason":"no"}}"#)))
        .unwrap();
    w.fold_last(&ev);
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
        assert_err(
            w.book.apply_entry(&approval_origin(&a), std::slice::from_ref(&e)),
            Code::Internal,
            "invariant_violated",
        );
        assert_eq!(w.record(r).state(), RequestState::Pending, "{name}");
    }
    // Two events for one request in one entry are refused, and neither is applied.
    let wd = Event::new(
        fid_bus::handler::EventType::new(REQUEST_WITHDRAWN).unwrap(),
        json(&format!(r#"{{"request":"{r}","withdrawn_by":{{"id":"claude-a","kind":"agent"}}}}"#)),
    );
    assert_err(w.book.apply_entry(&approval_origin(&a), &[good.clone(), wd]), Code::Internal, "invariant_violated");
    assert_eq!(w.record(r).state(), RequestState::Pending);
    w.book.apply_entry(&approval_origin(&a), &[good]).unwrap();
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
    w.fold_last(&events);
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
    w.fold_last(&events);
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
    w.fold_last(&ok);
    assert_eq!(w.record(r2).state(), RequestState::Rejected);
    // A forged rejection whose reason does not match its commitment is refused by the fold.
    let r3 = w.file(&agent, REQUEST_CMD, &filed_params(), 3);
    let mut b = obj(events[0].body()).clone();
    b.insert("request".into(), CValue::Str(r3.to_string()));
    b.insert("reason".into(), CValue::Str("other".into()));
    let forged = Event::new(events[0].type_id().clone(), CValue::Obj(b));
    assert_err(w.fold_last_result(&[forged]), Code::Internal, "invariant_violated");
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
    w.fold_last(&ev);
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
        book.apply_entry(&EntryOrigin::from_draft(d).unwrap(), &draft_events(d)).unwrap();
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
    w.fold_last(&ev);
    let ev = w
        .run(&alice, &format!("{REJECT_REQUEST}@1"), &json(&format!(r#"{{"request":"{r3}","reason":"wrong datum"}}"#)))
        .unwrap();
    w.fold_last(&ev);
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

// ------------------------------------------------------------------ review of #328

/// Check order: for an agent, projection and filing come before the class check, so a
/// human_only W2 and a W3 command become pending requests (the class check alone would
/// refuse them); the approving human passes the class check; outside MCP a script gets the
/// class refusals.
#[test]
fn projection_and_filing_come_before_the_class_check() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let alice = w.channel(ChannelKind::Gui, "alice");
    let bob = w.channel(ChannelKind::Cli, "bob");
    let p = json(r#"{"target":"plane-1"}"#);
    let agent_p = w.table.derive_principal(&agent).unwrap();
    let alice_p = w.table.derive_principal(&alice).unwrap();

    // human_only W2: the class check alone says human_only; over MCP it files.
    assert_err(authorise_class(&agent_p, &def(HUMAN_CMD)), Code::Policy, "human_only");
    let Decision::File(f) = w.authorise(&agent, HUMAN_CMD, &p, None).unwrap() else { panic!("human_only") };
    let r_human = w.commit_filing(&f, rid(1)).unwrap();
    let Decision::RunApproved(a) = w.authorise(&alice, HUMAN_CMD, &p, Some(&r_human)).unwrap() else { panic!() };
    assert_eq!(a.requested_by(), &agent_p.reference());

    // W3: the class check alone says class_forbidden; over MCP it files.
    let w3 = class_def("W3", "request_only", false);
    let vp = ValidParams::for_tests(p.clone());
    let decide = |w: &World, ch: &TestChannel, approval: Option<&RequestId>| {
        let view = w.book.view(HEAD);
        authorise(&w.table, ch, &w3, &vp, approval, &view).map(|a| a.into_parts().1)
    };
    assert_err(authorise_class(&agent_p, &w3), Code::Policy, "class_forbidden");
    let Decision::File(f) = decide(&w, &agent, None).unwrap() else { panic!("W3") };
    let r_w3 = w.commit_filing(&f, rid(2)).unwrap();
    assert_eq!(w.record(r_w3).state(), RequestState::Pending);
    // The approving human passes the class check, and the approval runs.
    authorise_class(&alice_p, &w3).unwrap();
    let Decision::RunApproved(a) = decide(&w, &alice, Some(&r_w3)).unwrap() else { panic!("approve W3") };
    assert_eq!(a.approved_by(), &alice_p.reference());
    // A script is refused the approval, and outside MCP gets the class refusals, not a request.
    assert_err(decide(&w, &bob, Some(&r_w3)), Code::Policy, "bad_approval");
    assert_err(decide(&w, &bob, None), Code::Policy, "class_forbidden");
    assert_err(w.authorise(&bob, HUMAN_CMD, &p, None), Code::Policy, "human_only");
    // M1 is never on MCP; an exposed W1 still runs for the agent after the class check.
    let m1 = class_def("M1", "never", false);
    let view = w.book.view(HEAD);
    assert_err(authorise(&w.table, &agent, &m1, &vp, None, &view), Code::NotFound, "unknown_command");
    assert_eq!(w.authorise(&agent, "test.run_exposed@1", &p, None).unwrap(), Decision::Run);
    assert_eq!(w.journal.drafts.len(), 2);
}

/// The fold takes the entry's own principal and command: a request event emitted by an
/// agent-runnable command, by the wrong principal or in the wrong command's entry is refused.
#[test]
fn the_fold_checks_the_entry_principal_and_command() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let alice = w.channel(ChannelKind::Gui, "alice");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let Decision::RunApproved(a) = w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r)).unwrap() else {
        panic!()
    };
    let approved = a.event().unwrap();
    let withdraw = format!("{WITHDRAW_REQUEST}@1");
    let reject = format!("{REJECT_REQUEST}@1");
    let wd = w.run(&agent, &withdraw, &json(&format!(r#"{{"request":"{r}"}}"#))).unwrap();
    let rj = w.run(&alice, &reject, &json(&format!(r#"{{"request":"{r}","reason":"no"}}"#))).unwrap();
    let Decision::File(f) = w.authorise(&agent, HUMAN_CMD, &filed_params(), None).unwrap() else { panic!() };
    let filed = f.event(rid(2)).unwrap();
    let carol = w.channel(ChannelKind::Gui, "carol");
    let cases: Vec<(&str, EntryOrigin, Event)> = vec![
        ("approval forged by an agent-runnable command", w.origin(&agent, "test.run_exposed@1"), approved.clone()),
        ("approval in the agent's entry of the approved command", w.origin(&agent, REQUEST_CMD), approved.clone()),
        ("approval by the human in another command's entry", w.origin(&alice, "test.run_exposed@1"), approved.clone()),
        ("approval by another human", w.origin(&carol, REQUEST_CMD), approved.clone()),
        ("withdrawal outside withdraw_request", w.origin(&agent, "test.run_exposed@1"), wd[0].clone()),
        ("withdrawal by another principal's entry", w.origin(&alice, &withdraw), wd[0].clone()),
        ("rejection outside reject_request", w.origin(&alice, "test.run_exposed@1"), rj[0].clone()),
        ("rejection in another principal's entry", w.origin(&agent, &reject), rj[0].clone()),
        ("filing outside file_request", w.origin(&agent, "test.run_exposed@1"), filed.clone()),
        ("filing in another principal's entry", w.origin(&alice, &format!("{FILE_REQUEST}@1")), filed.clone()),
    ];
    for (name, origin, e) in cases {
        let got = w.book.apply_entry(&origin, std::slice::from_ref(&e));
        assert_err(got, Code::Internal, "invariant_violated");
        assert_eq!(w.record(r).state(), RequestState::Pending, "{name}");
        assert!(w.book.get(&rid(2)).is_none(), "{name}");
    }
    // A parameters commitment that differs from the record is refused.
    let mut b = obj(approved.body()).clone();
    b.insert(
        "params_commitment".into(),
        CValue::Str(params_commitment(&json(r#"{"target":"x"}"#)).unwrap().as_str().into()),
    );
    let other = Event::new(approved.type_id().clone(), CValue::Obj(b));
    assert_err(w.book.apply_entry(&approval_origin(&a), &[other]), Code::Internal, "invariant_violated");
    // The genuine entries fold.
    w.book.apply_entry(&w.origin(&agent, &format!("{FILE_REQUEST}@1")), &[filed]).unwrap();
    w.book.apply_entry(&approval_origin(&a), &[approved]).unwrap();
    assert_eq!(w.record(r).state(), RequestState::Approved);
}

/// An agent-runnable command cannot emit a request event: the effects check refuses an
/// undeclared one, and the registry refuses declaring a reserved one on any other command.
#[test]
fn request_events_are_reserved_effects() {
    struct Forger(Event);
    impl Handler for Forger {
        fn run(
            &self,
            _view: &dyn StateView,
            _params: &ValidParams,
            _ctx: &HandlerCtx,
        ) -> Result<Vec<Event>, fid_bus::handler::HandlerError> {
            Ok(vec![self.0.clone()])
        }
    }
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let alice = w.channel(ChannelKind::Gui, "alice");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let Decision::RunApproved(a) = w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r)).unwrap() else {
        panic!()
    };
    let mut set = HandlerSet::new();
    set.register(def("test.run_exposed@1"), Box::new(Forger(a.event().unwrap()))).unwrap();
    let ctx = HandlerCtx::derive(&w.table, &agent, 0, CancelToken::never()).unwrap();
    let view = w.book.view(HEAD);
    let got = run_handler(&set, "test.run_exposed@1", &view, &ValidParams::for_tests(filed_params()), &ctx);
    assert_err(got, Code::UndeclaredEffect, "undeclared_event_type");

    for effect in ["RequestApproved", "RequestFiled", "RequestWithdrawn", "RequestRejected"] {
        let mut v = obj(&def("test.run_exposed@1").to_value()).clone();
        v.insert("effects".into(), CValue::Arr(vec![CValue::Str("TestDone".into()), CValue::Str(effect.into())]));
        let errs = fid_registry::validate(&[CommandDecl(CValue::Obj(v))]);
        let rules: Vec<(&str, Option<&str>)> = errs.iter().map(|e| (e.rule.as_str(), e.field.as_deref())).collect();
        assert_eq!(rules, [("reserved_effect", Some("/effects/1"))], "{effect}");
    }
    // The four request events; the branch events of CORE.JRN.BR.NAME are reserved too.
    assert_eq!(fid_registry::RESERVED_EFFECTS.iter().filter(|(n, _)| n.starts_with("Request")).count(), 4);
}

/// Races: two approvals of one request decided at the same head are both produced; each
/// carries that head. Only the commit stage (CORE.BUS.PIPE.STAGE, not built) can make sure
/// only one commits, by re-checking at append time; until then the fold refuses the second.
#[test]
fn two_approvals_at_one_head_are_both_produced() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let alice = w.channel(ChannelKind::Gui, "alice");
    let carol = w.channel(ChannelKind::Gui, "carol");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let Decision::RunApproved(a1) = w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r)).unwrap() else {
        panic!()
    };
    let Decision::RunApproved(a2) = w.authorise(&carol, REQUEST_CMD, &filed_params(), Some(&r)).unwrap() else {
        panic!()
    };
    assert_eq!(a1.decided_at_head(), a2.decided_at_head());
    assert_eq!(a1.decided_at_head().as_str(), HEAD.to_string());
    let Decision::File(f) = w.authorise(&agent, HUMAN_CMD, &filed_params(), None).unwrap() else { panic!() };
    assert_eq!(f.decided_at_head().as_str(), HEAD.to_string());
    w.book.apply_entry(&approval_origin(&a1), &[a1.event().unwrap()]).unwrap();
    assert_err(w.book.apply_entry(&approval_origin(&a2), &[a2.event().unwrap()]), Code::Internal, "invariant_violated");
    assert_eq!(w.record(r).resolved_by(), Some(a1.approved_by()));
}

/// Parameters that do not fit the target command's params_schema, or are oversized, cannot
/// be filed, and nothing is journaled.
#[test]
fn invalid_or_oversized_params_cannot_be_filed() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    for bad in [
        r#"{}"#,
        r#"{"count":3}"#,
        r#"{"target":7}"#,
        r#"{"target":"plane-1","count":"three"}"#,
        r#"{"target":"plane-1","count":1.5}"#,
        r#"{"target":"plane-1","extra":true}"#,
        r#"["plane-1"]"#,
    ] {
        assert_err(w.authorise(&agent, REQUEST_CMD, &json(bad), None), Code::Validation, "schema");
    }
    let mut big = Obj::new();
    big.insert("target".into(), CValue::Str("x".repeat(MAX_FILED_PARAMS_BYTES as usize)));
    assert_err(w.authorise(&agent, REQUEST_CMD, &CValue::Obj(big), None), Code::Validation, "input_limit");
    let mut fits = Obj::new();
    fits.insert("target".into(), CValue::Str("x".repeat(MAX_FILED_PARAMS_BYTES as usize - 64)));
    assert!(matches!(w.authorise(&agent, REQUEST_CMD, &CValue::Obj(fits), None), Ok(Decision::File(_))));
    assert!(w.journal.drafts.is_empty());
    assert!(w.book.is_empty());
}

/// A rejection reason with a C0 or C1 control, DEL or a bidi embedding, override or isolate
/// character is `validation/schema`.
#[test]
fn reject_reasons_refuse_control_and_bidi_characters() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let alice = w.channel(ChannelKind::Gui, "alice");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let cmd = format!("{REJECT_REQUEST}@1");
    let with = |reason: &str| {
        let mut o = Obj::new();
        o.insert("request".into(), CValue::Str(r.to_string()));
        o.insert("reason".into(), CValue::Str(reason.into()));
        CValue::Obj(o)
    };
    for bad in [
        "a\u{0}b",
        "line\nbreak",
        "tab\there",
        "cr\r",
        "\u{1f}",
        "del\u{7f}",
        "c1\u{85}",
        "\u{9f}",
        "x\u{202a}",
        "x\u{202b}",
        "x\u{202c}",
        "x\u{202d}",
        "evil\u{202e}txt",
        "\u{2066}x",
        "\u{2067}x",
        "\u{2068}x",
        "x\u{2069}",
        "\u{200f}rtl mark",
    ] {
        let e = w.run(&alice, &cmd, &with(bad)).unwrap_err();
        assert_eq!((e.code(), e.reason()), (Code::Validation, "schema"), "{bad:?}");
        assert!(e.detail().as_untrusted_str().contains("forbidden character"), "{bad:?}");
    }
    for ok in ["wrong datum", "Bezug falsch: \u{e9}\u{4e2d}", "a b", "nbsp\u{a0}", "\u{1f600}"] {
        assert!(!ok.chars().any(forbidden_in_reason), "{ok:?}");
    }
    assert_eq!(w.record(r).state(), RequestState::Pending);
    let ev = w.run(&alice, &cmd, &with("Bezug falsch: \u{e9}")).unwrap();
    w.fold_last(&ev);
    assert_eq!(w.record(r).state(), RequestState::Rejected);
}

/// The reject-reason filter, one case per range of its table (first and last code point),
/// every plane-end noncharacter, the reviewer's named code points, and every Unicode 16.0 Cf
/// range of the principal checks.
#[test]
fn reject_reason_filter_covers_every_range() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let alice = w.channel(ChannelKind::Gui, "alice");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let cmd = format!("{REJECT_REQUEST}@1");
    let refused = |w: &World, u: u32| {
        let c = char::from_u32(u).unwrap();
        let mut o = Obj::new();
        o.insert("request".into(), CValue::Str(r.to_string()));
        o.insert("reason".into(), CValue::Str(format!("x{c}y")));
        let e = w.run(&alice, &cmd, &CValue::Obj(o)).unwrap_err();
        assert_eq!((e.code(), e.reason()), (Code::Validation, "schema"), "U+{u:04X}");
        assert!(forbidden_in_reason(c), "U+{u:04X}");
    };
    assert_eq!(REASON_FORBIDDEN_RANGES.len(), 25);
    for (lo, hi, what) in REASON_FORBIDDEN_RANGES {
        assert!(lo <= hi, "{what}");
        refused(&w, *lo);
        refused(&w, *hi);
    }
    for plane in 0..=0x10u32 {
        refused(&w, (plane << 16) | 0xFFFE);
        refused(&w, (plane << 16) | 0xFFFF);
    }
    let named = [
        0x2028, 0x2029, 0xFEFF, 0x200B, 0x200C, 0x200D, 0x200E, 0x200F, 0x2060, 0x2061, 0x2062, 0x2063, 0x2064, 0x061C,
        0xE0000, 0xE0001, 0xE0020, 0xE007F, 0xFDD0, 0xFDEF,
    ];
    for u in named {
        refused(&w, u);
    }
    for (lo, hi) in fid_bus::FORMAT_RANGES {
        for u in [*lo, *hi] {
            assert!(forbidden_in_reason(char::from_u32(u).unwrap()), "Cf U+{u:04X}");
        }
    }
    for u in [0x20u32, 0x7E, 0xA0, 0xAC, 0xAE, 0x2027, 0x202F, 0x2065, 0xFDCF, 0xFDF0, 0xFFFD, 0xE0080, 0x1F600] {
        assert!(!forbidden_in_reason(char::from_u32(u).unwrap()), "U+{u:04X} allowed");
    }
    assert_eq!(w.record(r).state(), RequestState::Pending);
}

/// The fold compares the approving entry's own parameters commitment with the record's: a
/// `RequestApproved` in an entry of the right command and human but other parameters is
/// refused. `EntryOrigin::from_draft` takes it from the entry's arguments.
#[test]
fn the_fold_checks_the_entry_params_commitment() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let alice = w.channel(ChannelKind::Gui, "alice");
    let r = w.file(&agent, REQUEST_CMD, &filed_params(), 1);
    let Decision::RunApproved(a) = w.authorise(&alice, REQUEST_CMD, &filed_params(), Some(&r)).unwrap() else {
        panic!()
    };
    let alice_ref = w.table.derive_principal(&alice).unwrap().reference();
    let cmd = CommandId::parse(REQUEST_CMD).unwrap();
    let other = params_commitment(&json(r#"{"target":"plane-2","count":3}"#)).unwrap();
    let wrong = EntryOrigin::new(alice_ref.clone(), cmd.clone(), other);
    assert_err(w.book.apply_entry(&wrong, &[a.event().unwrap()]), Code::Internal, "invariant_violated");
    assert_eq!(w.record(r).state(), RequestState::Pending);
    // from_draft commits the entry's arguments.
    let draft = w.journal.drafts[0].clone();
    let DraftBody::Command(c) = &draft.body else { panic!() };
    assert_eq!(EntryOrigin::from_draft(&draft).unwrap().params_commitment(), params_commitment(&c.args).unwrap());
    let right = EntryOrigin::new(alice_ref, cmd, params_commitment(&filed_params()).unwrap());
    assert_eq!(right.params_commitment(), a.params_commitment());
    w.book.apply_entry(&right, &[a.event().unwrap()]).unwrap();
    assert_eq!(w.record(r).state(), RequestState::Approved);
}

/// D-035: over MCP a `never` command is refused exactly as a command the bus does not hold.
/// The refusal of each hidden command (declared `never`, no MCP surface, a `human_only` W1
/// declared `never`, and every class whose ceiling is `never`) is compared byte for byte, as
/// RFC 8785 JSON of code, reason and detail, with what `CommandTable::resolve` returns for the
/// same `id@major` from a table that does not hold it.
#[test]
fn a_never_command_over_mcp_is_refused_byte_for_byte_as_an_unknown_command() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-a");
    let agent_p = w.table.derive_principal(&agent).unwrap();
    let mut hidden: Vec<CommandDef> =
        ["test.run_never@1", "test.run_local@1", &format!("{REJECT_REQUEST}@1")].into_iter().map(def).collect();
    for class in ["M1", "S1", "A0"] {
        let mut d = class_def(class, "exposed", false);
        d.id = format!("test.class_{}", class.to_ascii_lowercase());
        hidden.push(d);
    }
    let visible = def("test.run_exposed@1");
    let table = fid_bus::validate::CommandTable::build(
        Digest([7; 32]),
        vec![visible.clone()],
        Catalogue::geo_frame().unwrap(),
        &fid_registry::ContextTypes::new(),
    )
    .unwrap();
    assert!(table.resolve(&visible.id, visible.major, &agent_p).is_ok());
    let view = w.book.view(HEAD);
    for d in &hidden {
        assert_eq!(mcp_projection(d), Projection::Never, "{}", d.id);
        let Err(unknown) = table.resolve(&d.id, d.major, &agent_p) else { panic!("{} resolved", d.id) };
        let unknown = unknown.to_json().unwrap();
        for kind in [ChannelKind::Mcp, ChannelKind::ServerMcp] {
            let e = authorise_projection(kind, d).unwrap_err();
            assert_eq!(e.to_json().unwrap(), unknown, "{} on {kind}", d.id);
        }
        let vp = ValidParams::for_tests(json("{}"));
        let e = authorise(&w.table, &agent, d, &vp, None, &view).unwrap_err();
        assert_eq!(e.to_json().unwrap(), unknown, "{} via authorise", d.id);
        // Nothing in the refusal names MCP or the projection.
        let text = String::from_utf8(unknown).unwrap();
        assert!(text.contains("unknown_command") && !text.contains("MCP") && !text.contains("not_on_mcp"), "{text}");
    }
    assert!(w.journal.drafts.is_empty());
    assert!(w.book.is_empty());
}
