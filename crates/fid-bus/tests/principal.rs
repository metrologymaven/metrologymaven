//! Acceptance criteria of spec node CORE.BUS.PRIN (AC-1 .. AC-8), with the hand-labelled
//! fixtures `fixtures/core/bus/principal/{channels,presence}.json`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{InternalReason, UpgradeRefusedReason};
use fid_bus::principal::{
    Channel, ChannelKind, ClientId, ConfirmMethod, HumanPresence, IdError, Identity, JournalAppend, OsUser,
    PresenceOutcome, PrincipalId, PrincipalKind, SelfAsserted, SessionId, SessionTable, UpgradeClock, UpgradeContext,
    UpgradeRequest, MAX_ID_BYTES, MAX_SELF_ASSERTED_CHARS, MAX_UPGRADED_SESSIONS, UPGRADE_DEADLINE,
};
use fid_bus::{BusError, Code, Detail};
use fid_canon::{parse, CValue, Digest};
use fid_journal::{
    build, verify_commitment, AiAssistance, CommandDraft, DraftBody, Entry, EntryDraft, EntryKind, EntryTime, Field,
    HeaderBody, Manifests, OsSalts, FIRST_SEQ, GENESIS_PREV,
};
use fid_types::{Determinism, SideEffect};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

type Obj = BTreeMap<String, CValue>;

fn fixture(name: &str) -> Obj {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/bus/principal").join(name);
    match parse(&std::fs::read(&path).unwrap()).unwrap() {
        CValue::Obj(o) => o,
        other => panic!("{name}: object expected, got {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> Vec<&'a Obj> {
    match o.get(k) {
        Some(CValue::Arr(items)) => items
            .iter()
            .map(|v| match v {
                CValue::Obj(m) => m,
                other => panic!("{k}: object expected, got {other:?}"),
            })
            .collect(),
        other => panic!("field {k} is {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("field {k} is {other:?}"),
    }
}

fn opt_st<'a>(o: &'a Obj, k: &str) -> Option<&'a str> {
    match o.get(k) {
        None | Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(s),
        other => panic!("field {k} is {other:?}"),
    }
}

fn pid(s: &str) -> PrincipalId {
    PrincipalId::new(s).unwrap()
}

fn sid(s: &str) -> SessionId {
    SessionId::new(s).unwrap()
}

fn user(s: &str) -> OsUser {
    OsUser::new(s).unwrap()
}

// ------------------------------------------------------------------ test channel

#[derive(Debug, Clone)]
struct TestChannel {
    kind: ChannelKind,
    identity: Option<Identity>,
    session: SessionId,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        self.identity.clone()
    }
    fn session(&self) -> SessionId {
        self.session.clone()
    }
}

fn identity_from(v: Option<&CValue>) -> Option<Identity> {
    let o = match v {
        None | Some(CValue::Null) => return None,
        Some(CValue::Obj(o)) => o,
        other => panic!("identity is {other:?}"),
    };
    let mut id = Identity::new(pid(st(o, "subject")));
    id.os_user = opt_st(o, "os_user").map(user);
    id.acting_for = opt_st(o, "acting_for").map(pid);
    id.client_id = opt_st(o, "client_id").map(|c| ClientId::new(c).unwrap());
    if let Some(CValue::Obj(sa)) = o.get("self_asserted") {
        id.self_asserted = Some(SelfAsserted::untrusted(opt_st(sa, "client_name"), opt_st(sa, "model_name")));
    }
    Some(id)
}

fn channel_from(o: &Obj) -> TestChannel {
    TestChannel {
        kind: ChannelKind::from_wire(st(o, "kind")).unwrap(),
        identity: identity_from(o.get("identity")),
        session: sid(st(o, "session")),
    }
}

fn cli(session: &str, os_user: &str) -> TestChannel {
    let mut id = Identity::new(pid(os_user));
    id.os_user = Some(user(os_user));
    TestChannel { kind: ChannelKind::Cli, identity: Some(id), session: sid(session) }
}

fn with_kind(mut ch: TestChannel, kind: ChannelKind) -> TestChannel {
    ch.kind = kind;
    ch
}

// ------------------------------------------------------------------ fakes

struct FakePresence {
    outcome: PresenceOutcome,
    calls: Cell<u32>,
    deadline: Cell<Option<Duration>>,
    seen: RefCell<Option<UpgradeRequest>>,
}

impl FakePresence {
    fn new(outcome: PresenceOutcome) -> Self {
        FakePresence { outcome, calls: Cell::new(0), deadline: Cell::new(None), seen: RefCell::new(None) }
    }
    fn confirms(os_user: &str, method: ConfirmMethod) -> Self {
        FakePresence::new(PresenceOutcome::Confirmed { os_user: user(os_user), method })
    }
}

impl HumanPresence for FakePresence {
    fn confirm(&self, req: &UpgradeRequest, deadline: Duration) -> PresenceOutcome {
        self.calls.set(self.calls.get() + 1);
        self.deadline.set(Some(deadline));
        *self.seen.borrow_mut() = Some(req.clone());
        self.outcome.clone()
    }
}

/// Each call to `now` advances the monotonic clock by the next step (nanoseconds).
struct FakeClock {
    mono: i128,
    steps: Vec<i128>,
    calls: usize,
}

impl FakeClock {
    /// The prompt takes `elapsed_ms`; every other step is 1 ms.
    fn prompt_takes(elapsed_ms: i128) -> Self {
        FakeClock { mono: 5_000_000_000, steps: vec![0, elapsed_ms * 1_000_000], calls: 0 }
    }
}

impl UpgradeClock for FakeClock {
    fn now(&mut self) -> EntryTime {
        self.mono += self.steps.get(self.calls).copied().unwrap_or(1_000_000);
        self.calls += 1;
        EntryTime {
            utc: format!("2026-11-19T10:00:{:02}Z", self.calls.min(59)),
            mono_ns: u64::try_from(self.mono).unwrap(),
            sync: "ntp".into(),
        }
    }
}

/// An in-memory Record Journal that builds real entries with `fid-journal`.
#[derive(Default)]
struct MemJournal {
    entries: Vec<Entry>,
    fail: bool,
}

impl JournalAppend for MemJournal {
    fn append(&mut self, draft: &EntryDraft) -> Result<(), BusError> {
        if self.fail {
            return Err(BusError::Internal(InternalReason::JournalUnavailable, Detail::new("test journal is down")));
        }
        let (seq, prev) = match self.entries.last() {
            Some(e) => (e.header.seq + 1, e.header.hash),
            None => (FIRST_SEQ, GENESIS_PREV),
        };
        let entry = build(draft, seq, prev, &mut OsSalts)
            .map_err(|e| BusError::Internal(InternalReason::InvariantViolated, Detail::new(&e.to_string())))?;
        self.entries.push(entry);
        Ok(())
    }
}

/// Counts appends without building entries (capacity test).
#[derive(Default)]
struct CountingJournal(usize);

impl JournalAppend for CountingJournal {
    fn append(&mut self, _draft: &EntryDraft) -> Result<(), BusError> {
        self.0 += 1;
        Ok(())
    }
}

fn manifests() -> Manifests {
    Manifests { registry: Digest([0x11; 32]), kernel: Digest([0x22; 32]) }
}

fn ctx() -> UpgradeContext {
    UpgradeContext { stream: "record".into(), branch: "main".into(), manifests: manifests() }
}

/// The pipeline stand-in for AC-4 and AC-7: derive the principal from the channel, then
/// journal one W2 test command under it. An error journals nothing.
fn invoke_w2(table: &SessionTable, ch: &dyn Channel, journal: &mut MemJournal) -> Result<(), BusError> {
    let principal = table.derive_principal(ch)?;
    let draft = EntryDraft {
        stream: "record".into(),
        branch: "main".into(),
        logical_parent: None,
        time: EntryTime { utc: "2026-11-19T10:01:00Z".into(), mono_ns: 9_000_000_000, sync: "ntp".into() },
        principal: principal.to_draft(),
        ai_assistance: AiAssistance::None,
        manifests: manifests(),
        reason: None,
        human_assertion: None,
        body: DraftBody::Command(CommandDraft {
            id: "test.touch_value".into(),
            major: 1,
            class: SideEffect::W2,
            determinism: Determinism::D0,
            seed: 0,
            args: CValue::Obj(BTreeMap::new()),
            events: vec![],
            inputs: vec![],
            consumed_r1: vec![],
            outputs: vec![],
            algorithms: vec![],
        }),
    };
    journal.append(&draft)
}

fn code_reason(e: &BusError) -> String {
    format!("{}/{}", e.code().as_str(), e.reason())
}

// ------------------------------------------------------------------ AC-1

/// AC-1 as written in the spec, in its order: channel -> principal kind.
const AC1: [(&str, &str); 11] = [
    ("gui", "human"),
    ("cli", "script"),
    ("python", "script"),
    ("control", "script"),
    ("mcp", "agent"),
    ("server_mcp", "agent"),
    ("service", "service"),
    ("device", "device"),
    ("internal", "system"),
    ("server_session", "script"),
    ("server_service", "service"),
];

#[test]
fn ac1_every_channel_kind_maps_as_labelled() {
    let fx = fixture("channels.json");
    assert_eq!(st(&fx, "license"), "CC-BY-4.0");
    let channels = arr(&fx, "channels");
    assert_eq!(channels.len(), AC1.len());
    let table = SessionTable::new();
    for (o, (kind, expect_kind)) in channels.iter().zip(AC1) {
        assert_eq!(st(o, "kind"), kind, "fixture order follows AC-1");
        let expect = match o.get("expect") {
            Some(CValue::Obj(e)) => e,
            other => panic!("expect is {other:?}"),
        };
        assert_eq!(st(expect, "kind"), expect_kind, "fixture label agrees with AC-1 for {kind}");
        let p = table.derive_principal(&channel_from(o)).unwrap();
        assert_eq!(p.kind(), PrincipalKind::parse(expect_kind).unwrap(), "{kind}");
        assert_eq!(p.id().as_str(), st(expect, "id"), "{kind}");
        assert_eq!(p.on_behalf_of().map(PrincipalId::as_str), opt_st(expect, "on_behalf_of"), "{kind}");
        assert_eq!(p.upgraded_session(), None, "{kind}");
    }
    // One fixture channel per kind, and every kind is covered.
    let kinds: BTreeSet<&str> = channels.iter().map(|o| st(o, "kind")).collect();
    let all: BTreeSet<&str> = ChannelKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(kinds, all);
}

#[test]
fn req013_base_mapping_is_total_and_matches_ac1() {
    assert_eq!(ChannelKind::ALL.len(), 11);
    for (wire, kind) in AC1 {
        let ck = ChannelKind::from_wire(wire).unwrap();
        assert_eq!(ck.as_str(), wire);
        assert_eq!(ck.base_kind().as_str(), kind);
    }
    let distinct: BTreeSet<&str> = ChannelKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(distinct.len(), ChannelKind::ALL.len());
    assert_eq!(ChannelKind::from_wire("CLI"), None);
    assert_eq!(ChannelKind::from_wire("unknown"), None);
    let upgradable: Vec<ChannelKind> = ChannelKind::ALL.iter().copied().filter(|k| k.upgradable()).collect();
    assert_eq!(upgradable, vec![ChannelKind::Cli, ChannelKind::Python]);
}

#[test]
fn req001_exactly_six_principal_kinds() {
    let kinds: Vec<&str> = PrincipalKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(kinds, ["human", "agent", "script", "service", "device", "system"]);
}

#[test]
fn acting_for_is_ignored_outside_server_session_and_internal() {
    let fx = fixture("channels.json");
    let table = SessionTable::new();
    for o in arr(&fx, "acting_for_ignored") {
        let expect = match o.get("expect") {
            Some(CValue::Obj(e)) => e,
            other => panic!("expect is {other:?}"),
        };
        let p = table.derive_principal(&channel_from(o)).unwrap();
        assert_eq!(p.kind().as_str(), st(expect, "kind"), "{}", st(o, "name"));
        assert_eq!(p.id().as_str(), st(expect, "id"));
        assert_eq!(p.on_behalf_of(), None, "{}", st(o, "name"));
    }
}

// ------------------------------------------------------------------ AC-2, REQ-007

#[test]
fn ac2_params_claiming_human_have_no_effect() {
    let fx = fixture("channels.json");
    let channels = arr(&fx, "channels");
    let mcp = channels.iter().find(|o| st(o, "name") == "mcp").unwrap();
    // The fixture's params really claim `human`.
    let Some(CValue::Obj(params)) = mcp.get("params") else { panic!("params") };
    let Some(CValue::Obj(claimed)) = params.get("principal") else { panic!("params.principal") };
    assert_eq!(st(claimed, "kind"), "human");
    // derive_principal takes the channel only, so params cannot reach it.
    let table = SessionTable::new();
    let p = table.derive_principal(&channel_from(mcp)).unwrap();
    assert_eq!(p.kind(), PrincipalKind::Agent);
    assert_eq!(p.to_draft().kind, PrincipalKind::Agent);
    assert_eq!(p.to_draft().identity, CValue::Str("mcp-client:desktop-assistant".into()));
    // Every channel in the fixture carries params; none changes the labelled result
    // (checked in AC-1), and the same channel derives identically on every call.
    for o in &channels {
        let ch = channel_from(o);
        assert_eq!(table.derive_principal(&ch).unwrap(), table.derive_principal(&ch).unwrap());
    }
}

#[test]
fn req007_self_asserted_names_never_change_the_principal() {
    let base = |sa| {
        let mut id = Identity::new(pid("mcp-client:x"));
        id.self_asserted = sa;
        TestChannel { kind: ChannelKind::Mcp, identity: Some(id), session: sid("mcp-1") }
    };
    let table = SessionTable::new();
    let plain = table.derive_principal(&base(None)).unwrap();
    for (client, model) in [(Some("human"), Some("human")), (Some("GUI"), None), (None, Some("system")), (Some(""), Some(""))] {
        let p = table.derive_principal(&base(Some(SelfAsserted::untrusted(client, model)))).unwrap();
        assert_eq!(p.kind(), plain.kind());
        assert_eq!(p.id(), plain.id());
        assert_eq!(p.on_behalf_of(), plain.on_behalf_of());
        assert_eq!(p.to_draft(), plain.to_draft());
        let sa = p.self_asserted().unwrap().as_untrusted();
        assert_eq!(sa.client_name(), client);
        assert_eq!(sa.model_name(), model);
    }
    // Self-asserted text is bounded: cut to MAX_SELF_ASSERTED_CHARS characters.
    let long = "\u{e9}".repeat(MAX_SELF_ASSERTED_CHARS + 10);
    let sa = SelfAsserted::untrusted(Some(&long), Some("m"));
    assert_eq!(sa.as_untrusted().client_name().unwrap().chars().count(), MAX_SELF_ASSERTED_CHARS);
    assert_eq!(sa.as_untrusted().model_name(), Some("m"));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_cli_and_python_without_upgrade_are_script() {
    let table = SessionTable::new();
    for kind in [ChannelKind::Cli, ChannelKind::Python, ChannelKind::Control] {
        let p = table.derive_principal(&with_kind(cli("s-1", "alice"), kind)).unwrap();
        assert_eq!(p.kind(), PrincipalKind::Script, "{kind}");
        assert_eq!(p.id().as_str(), "alice");
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_confirmed_upgrade_journals_then_runs_as_human() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let ch = cli("cli-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    table.upgrade_session(&ch, &presence, &mut journal, &mut FakeClock::prompt_takes(4000), &ctx()).unwrap();
    assert_eq!(presence.calls.get(), 1);
    assert_eq!(presence.deadline.get(), Some(Duration::from_secs(120)));
    let req = presence.seen.borrow().clone().unwrap();
    assert_eq!((req.session.as_str(), req.channel, req.os_user.as_str()), ("cli-1", ChannelKind::Cli, "alice"));

    invoke_w2(&table, &ch, &mut journal).unwrap();
    assert_eq!(journal.entries.len(), 2);

    // Entry 1: principal_upgrade with session ID, method, identity commitment and time.
    let up = &journal.entries[0];
    assert_eq!(up.header.kind(), EntryKind::PrincipalUpgrade);
    assert_eq!(up.header.principal.kind, PrincipalKind::Script, "the request comes from the script session");
    let HeaderBody::PrincipalUpgrade(u) = &up.header.body else { panic!("upgrade body") };
    assert_eq!(u.session_id, "cli-1");
    assert_eq!(u.method, ConfirmMethod::GuiPrompt);
    assert_eq!(u.confirmed_at, "2026-11-19T10:00:02Z");
    assert_eq!(up.header.time.utc, u.confirmed_at);
    let alice = CValue::Str("alice".into());
    let salt = up.payload.salts.get(&Field::UpgradeIdentity);
    verify_commitment(&up.header, Field::UpgradeIdentity, salt, Some(&alice)).unwrap();
    let mallory = CValue::Str("mallory".into());
    assert!(verify_commitment(&up.header, Field::UpgradeIdentity, salt, Some(&mallory)).is_err());

    // Entry 2: the W2 command, principal kind human, identity the confirming OS user.
    let w2 = &journal.entries[1];
    assert_eq!(w2.header.kind(), EntryKind::Command);
    assert_eq!(w2.header.principal.kind, PrincipalKind::Human);
    assert_eq!(w2.header.prev, up.header.hash);
    let salt = w2.payload.salts.get(&Field::Identity);
    verify_commitment(&w2.header, Field::Identity, salt, Some(&alice)).unwrap();
    let HeaderBody::Command(c) = &w2.header.body else { panic!("command body") };
    assert_eq!(c.class, SideEffect::W2);

    let p = table.derive_principal(&ch).unwrap();
    assert_eq!(p.kind(), PrincipalKind::Human);
    assert_eq!(p.upgraded_session().map(SessionId::as_str), Some("cli-1"));

    // A later session of the same process starts as script again.
    assert_eq!(table.derive_principal(&cli("cli-2", "alice")).unwrap().kind(), PrincipalKind::Script);
    // Ending the session removes the row; reusing its ID is script.
    assert!(table.end_session(&sid("cli-1")));
    assert!(!table.end_session(&sid("cli-1")));
    assert_eq!(table.derive_principal(&ch).unwrap().kind(), PrincipalKind::Script);
    assert!(table.is_empty());
    // A fresh table (a restart) never knows the session.
    assert_eq!(SessionTable::new().derive_principal(&ch).unwrap().kind(), PrincipalKind::Script);
}

#[test]
fn ac4_python_session_upgrades_with_os_presence() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let ch = with_kind(cli("py-1", "alice"), ChannelKind::Python);
    let presence = FakePresence::confirms("alice", ConfirmMethod::OsPresence);
    table.upgrade_session(&ch, &presence, &mut journal, &mut FakeClock::prompt_takes(10), &ctx()).unwrap();
    let HeaderBody::PrincipalUpgrade(u) = &journal.entries[0].header.body else { panic!("upgrade body") };
    assert_eq!(u.method, ConfirmMethod::OsPresence);
    assert_eq!(table.derive_principal(&ch).unwrap().kind(), PrincipalKind::Human);
}

#[test]
fn upgraded_session_is_human_only_for_same_channel_kind_and_os_user() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let ch = cli("s-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    table.upgrade_session(&ch, &presence, &mut journal, &mut FakeClock::prompt_takes(1), &ctx()).unwrap();
    // Another OS user presenting the same session ID stays script.
    assert_eq!(table.derive_principal(&cli("s-1", "bob")).unwrap().kind(), PrincipalKind::Script);
    // Same session ID on another channel kind keeps that channel's mapping.
    let py = with_kind(ch.clone(), ChannelKind::Python);
    assert_eq!(table.derive_principal(&py).unwrap().kind(), PrincipalKind::Script);
    let mcp = with_kind(ch.clone(), ChannelKind::Mcp);
    assert_eq!(table.derive_principal(&mcp).unwrap().kind(), PrincipalKind::Agent);
    let ctl = with_kind(ch.clone(), ChannelKind::Control);
    assert_eq!(table.derive_principal(&ctl).unwrap().kind(), PrincipalKind::Script);
    // A session whose identity has no OS user stays script.
    let mut no_user = ch.clone();
    no_user.identity.as_mut().unwrap().os_user = None;
    assert_eq!(table.derive_principal(&no_user).unwrap().kind(), PrincipalKind::Script);
}

#[test]
fn second_upgrade_of_an_upgraded_session_neither_prompts_nor_journals() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let ch = cli("s-1", "alice");
    let first = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    table.upgrade_session(&ch, &first, &mut journal, &mut FakeClock::prompt_takes(1), &ctx()).unwrap();
    let second = FakePresence::new(PresenceOutcome::Declined);
    table.upgrade_session(&ch, &second, &mut journal, &mut FakeClock::prompt_takes(1), &ctx()).unwrap();
    assert_eq!(second.calls.get(), 0);
    assert_eq!(journal.entries.len(), 1);
}

#[test]
fn req011_journal_failure_leaves_the_session_script() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal { fail: true, ..MemJournal::default() };
    let ch = cli("s-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    let e = table.upgrade_session(&ch, &presence, &mut journal, &mut FakeClock::prompt_takes(1), &ctx()).unwrap_err();
    assert_eq!(code_reason(&e), "internal/journal_unavailable");
    assert!(table.is_empty());
    assert_eq!(table.derive_principal(&ch).unwrap().kind(), PrincipalKind::Script);
}

// ------------------------------------------------------------------ AC-5

fn outcome_from(o: &Obj) -> PresenceOutcome {
    match st(o, "result") {
        "confirmed" => PresenceOutcome::Confirmed {
            os_user: user(st(o, "os_user")),
            method: match st(o, "method") {
                "gui_prompt" => ConfirmMethod::GuiPrompt,
                "os_presence" => ConfirmMethod::OsPresence,
                other => panic!("method {other}"),
            },
        },
        "declined" => PresenceOutcome::Declined,
        "timed_out" => PresenceOutcome::TimedOut,
        other => panic!("result {other}"),
    }
}

fn elapsed_ms(o: &Obj) -> i128 {
    match o.get("elapsed_ms") {
        Some(CValue::Num(n)) => *n as i128,
        Some(CValue::I64(n)) => i128::from(*n),
        Some(CValue::U64(n)) => i128::from(*n),
        other => panic!("elapsed_ms is {other:?}"),
    }
}

#[test]
fn ac5_presence_outcomes_as_labelled() {
    let fx = fixture("presence.json");
    let os_user = st(&fx, "session_os_user");
    let cases = arr(&fx, "cases");
    assert_eq!(cases.len(), 9);
    let mut refused = 0;
    for case in cases {
        let name = st(case, "name");
        let Some(CValue::Obj(outcome)) = case.get("outcome") else { panic!("{name}: outcome") };
        let presence = FakePresence::new(outcome_from(outcome));
        let mut table = SessionTable::new();
        let mut journal = MemJournal::default();
        let ch = cli("cli-5", os_user);
        let mut clock = FakeClock::prompt_takes(elapsed_ms(case));
        let got = table.upgrade_session(&ch, &presence, &mut journal, &mut clock, &ctx());
        assert_eq!(presence.calls.get(), 1, "{name}");
        match st(case, "expect") {
            "ok" => {
                got.unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(journal.entries.len(), 1, "{name}");
                assert_eq!(table.derive_principal(&ch).unwrap().kind(), PrincipalKind::Human, "{name}");
            }
            want => {
                refused += 1;
                let e = got.expect_err(name);
                assert_eq!(e.code(), Code::UpgradeRefused, "{name}");
                assert_eq!(code_reason(&e), want, "{name}");
                assert!(journal.entries.is_empty(), "{name}: journal unchanged");
                assert!(table.is_empty(), "{name}");
                assert_eq!(table.derive_principal(&ch).unwrap().kind(), PrincipalKind::Script, "{name}");
            }
        }
    }
    assert_eq!(refused, 6);
}

#[test]
fn ac5_reasons_are_the_closed_upgrade_refused_set() {
    let reasons: Vec<&str> = UpgradeRefusedReason::ALL.iter().map(|r| r.as_str()).collect();
    assert_eq!(reasons, ["channel", "declined", "timeout", "user_mismatch"]);
    assert_eq!(UPGRADE_DEADLINE, Duration::from_secs(120));
}

#[test]
fn session_without_os_user_cannot_upgrade() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let mut ch = cli("s-1", "alice");
    ch.identity.as_mut().unwrap().os_user = None;
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    let e = table.upgrade_session(&ch, &presence, &mut journal, &mut FakeClock::prompt_takes(1), &ctx()).unwrap_err();
    assert_eq!(code_reason(&e), "upgrade_refused/user_mismatch");
    assert_eq!(presence.calls.get(), 0);
    assert!(journal.entries.is_empty());
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_upgrade_on_other_channels_is_refused_without_prompt() {
    let fx = fixture("channels.json");
    let mut checked = BTreeSet::new();
    for o in arr(&fx, "channels") {
        let ch = channel_from(o);
        if ch.kind.upgradable() {
            continue;
        }
        let mut table = SessionTable::new();
        let mut journal = MemJournal::default();
        let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
        let e = table
            .upgrade_session(&ch, &presence, &mut journal, &mut FakeClock::prompt_takes(1), &ctx())
            .expect_err(ch.kind.as_str());
        assert_eq!(code_reason(&e), "upgrade_refused/channel", "{}", ch.kind);
        assert_eq!(presence.calls.get(), 0, "{}: no prompt shown", ch.kind);
        assert!(journal.entries.is_empty());
        assert!(table.is_empty());
        checked.insert(ch.kind);
    }
    assert_eq!(checked.len(), 9);
    assert!(checked.contains(&ChannelKind::Mcp) && checked.contains(&ChannelKind::ServerSession));
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_channel_without_identity_is_unauthenticated_and_journals_nothing() {
    let fx = fixture("channels.json");
    let cases = arr(&fx, "unauthenticated");
    assert_eq!(cases.len(), 5);
    for o in cases {
        let name = st(o, "name");
        let ch = channel_from(o);
        let mut table = SessionTable::new();
        let mut journal = MemJournal::default();
        let e = table.derive_principal(&ch).expect_err(name);
        assert_eq!(code_reason(&e), "unauthenticated/no_identity", "{name}");
        let e = invoke_w2(&table, &ch, &mut journal).expect_err(name);
        assert_eq!(e.code(), Code::Unauthenticated, "{name}");
        assert!(journal.entries.is_empty(), "{name}: journals nothing");
        let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
        let e = table
            .upgrade_session(&ch, &presence, &mut journal, &mut FakeClock::prompt_takes(1), &ctx())
            .expect_err(name);
        assert_eq!(e.code(), Code::Unauthenticated, "{name}");
        assert_eq!(presence.calls.get(), 0, "{name}");
        assert!(journal.entries.is_empty(), "{name}");
    }
    let json = String::from_utf8(
        BusError::Unauthenticated(fid_bus::error::UnauthenticatedReason::NoIdentity, Detail::empty())
            .to_json()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json, r#"{"code":"unauthenticated","detail":"","reason":"no_identity"}"#);
}

// ------------------------------------------------------------------ AC-8

#[test]
fn ac8_internal_recompute_names_the_triggering_human() {
    let table = SessionTable::new();
    let gui = TestChannel {
        kind: ChannelKind::Gui,
        identity: Some({
            let mut id = Identity::new(pid("alice"));
            id.os_user = Some(user("alice"));
            id
        }),
        session: sid("desk-1"),
    };
    let human = table.derive_principal(&gui).unwrap();
    assert_eq!(human.kind(), PrincipalKind::Human);
    let mut id = Identity::new(pid("core:recompute"));
    id.acting_for = Some(human.id().clone());
    let internal = TestChannel { kind: ChannelKind::Internal, identity: Some(id), session: sid("core-1") };
    let p = table.derive_principal(&internal).unwrap();
    assert_eq!(p.kind(), PrincipalKind::System);
    assert_eq!(p.on_behalf_of(), Some(human.id()));
    assert_eq!(p.to_draft().on_behalf_of, Some(CValue::Str("alice".into())));
}

// ------------------------------------------------------------------ bounds

#[test]
fn identifiers_are_bounded_and_case_significant() {
    assert_eq!(PrincipalId::new(""), Err(IdError::Empty));
    assert_eq!(OsUser::new(&"a".repeat(MAX_ID_BYTES + 1)), Err(IdError::TooLong));
    assert!(SessionId::new(&"a".repeat(MAX_ID_BYTES)).is_ok());
    assert_eq!(ClientId::new("a\nb"), Err(IdError::ControlChar));
    assert_eq!(OsUser::new("bob\u{7f}"), Err(IdError::ControlChar));
    assert_eq!(IdError::TooLong.code(), "too_long");
    // OS user names compare byte for byte: a case-only difference is another user.
    assert_ne!(user("Alice"), user("alice"));
    assert_eq!(PrincipalId::from(&user("alice")).as_str(), "alice");
}

#[test]
fn session_table_is_bounded() {
    let mut table = SessionTable::new();
    let mut journal = CountingJournal::default();
    let presence = FakePresence::confirms("alice", ConfirmMethod::OsPresence);
    for i in 0..MAX_UPGRADED_SESSIONS {
        let ch = cli(&format!("s-{i}"), "alice");
        table.upgrade_session(&ch, &presence, &mut journal, &mut FakeClock::prompt_takes(1), &ctx()).unwrap();
    }
    assert_eq!(table.len(), MAX_UPGRADED_SESSIONS);
    let calls = presence.calls.get();
    let e = table
        .upgrade_session(&cli("one-more", "alice"), &presence, &mut journal, &mut FakeClock::prompt_takes(1), &ctx())
        .unwrap_err();
    assert_eq!(code_reason(&e), "internal/storage");
    assert_eq!(presence.calls.get(), calls, "no prompt when the table is full");
    assert_eq!(journal.0, MAX_UPGRADED_SESSIONS);
    // Ending a session frees a row.
    assert!(table.end_session(&sid("s-0")));
    table
        .upgrade_session(&cli("one-more", "alice"), &presence, &mut journal, &mut FakeClock::prompt_takes(1), &ctx())
        .unwrap();
}
