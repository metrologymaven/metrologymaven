//! Acceptance criteria of spec node CORE.BUS.PRIN (AC-1 .. AC-8), with the hand-labelled
//! fixtures `fixtures/core/bus/principal/{channels,presence,ids}.json`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{InternalReason, UnauthenticatedReason, UpgradeRefusedReason};
use fid_bus::principal::{
    Channel, ChannelKind, ClientId, ConfirmMethod, HumanPresence, IdError, Identity, JournalAppend, OsUser,
    PresenceOutcome, PrincipalId, PrincipalKind, PrincipalRef, SelfAsserted, SessionId, SessionKey, SessionTable,
    UpgradeClock, UpgradeContext, UpgradeGrant, UpgradeRequest, BLANK_CHARS, DEFAULT_IGNORABLE_RANGES, FORMAT_RANGES,
    MAX_ID_BYTES, MAX_SELF_ASSERTED_CHARS, MAX_UPGRADED_SESSIONS, PRIVATE_USE_RANGES, UNASSIGNED_RANGES,
    UPGRADE_DEADLINE,
};
use fid_bus::{BusError, Code, Detail, Untrusted};
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

fn ob<'a>(o: &'a Obj, k: &str) -> &'a Obj {
    match o.get(k) {
        Some(CValue::Obj(m)) => m,
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

fn pref(o: &Obj) -> PrincipalRef {
    PrincipalRef { kind: PrincipalKind::parse(st(o, "kind")).unwrap(), id: pid(st(o, "id")) }
}

fn opt_ref(o: &Obj, k: &str) -> Option<PrincipalRef> {
    match o.get(k) {
        None | Some(CValue::Null) => None,
        Some(CValue::Obj(m)) => Some(pref(m)),
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

// ------------------------------------------------------------------ test channels

#[derive(Debug)]
struct TestChannel {
    kind: ChannelKind,
    identity: Option<Identity>,
    session: SessionKey,
    grant: Option<UpgradeGrant>,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        self.identity.clone()
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        self.grant.as_ref()
    }
}

/// A channel that presents somebody else's session key and grant by reference: the most
/// an in-process forger can do, since neither type can be cloned or built.
struct Borrowing<'a> {
    kind: ChannelKind,
    identity: Identity,
    session: &'a SessionKey,
    grant: Option<&'a UpgradeGrant>,
}

impl Channel for Borrowing<'_> {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(self.identity.clone())
    }
    fn session(&self) -> &SessionKey {
        self.session
    }
    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        self.grant
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
    id.acting_for = opt_ref(o, "acting_for");
    id.client_id = opt_st(o, "client_id").map(|c| ClientId::new(c).unwrap());
    if let Some(CValue::Obj(sa)) = o.get("self_asserted") {
        id.self_asserted = Some(SelfAsserted::untrusted(opt_st(sa, "client_name"), opt_st(sa, "model_name")));
    }
    Some(id)
}

fn channel_from(table: &mut SessionTable, o: &Obj) -> TestChannel {
    TestChannel {
        kind: ChannelKind::from_wire(st(o, "kind")).unwrap(),
        identity: identity_from(o.get("identity")),
        session: table.open_session(sid(st(o, "session"))).unwrap(),
        grant: None,
    }
}

fn with_identity(table: &mut SessionTable, kind: ChannelKind, label: &str, id: Identity) -> TestChannel {
    TestChannel { kind, identity: Some(id), session: table.open_session(sid(label)).unwrap(), grant: None }
}

fn local(table: &mut SessionTable, kind: ChannelKind, label: &str, os_user: &str) -> TestChannel {
    let mut id = Identity::new(pid(os_user));
    id.os_user = Some(user(os_user));
    with_identity(table, kind, label, id)
}

fn cli(table: &mut SessionTable, label: &str, os_user: &str) -> TestChannel {
    local(table, ChannelKind::Cli, label, os_user)
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

/// The three upgrade steps in a row: prepare (`&self`), confirm (no table borrow), commit.
fn upgrade_with(
    table: &mut SessionTable,
    ch: &dyn Channel,
    presence: &dyn HumanPresence,
    journal: &mut dyn JournalAppend,
    clock: &mut FakeClock,
) -> Result<UpgradeGrant, BusError> {
    let pending = table.prepare_upgrade(ch)?;
    let confirmed = pending.confirm(presence, clock)?;
    table.commit_upgrade(confirmed, journal, &ctx())
}

/// Upgrade with a prompt that takes 1 ms.
fn upgrade(
    table: &mut SessionTable,
    ch: &dyn Channel,
    presence: &FakePresence,
    journal: &mut dyn JournalAppend,
) -> Result<UpgradeGrant, BusError> {
    upgrade_with(table, ch, presence, journal, &mut FakeClock::prompt_takes(1))
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

fn kind_of(table: &SessionTable, ch: &dyn Channel) -> PrincipalKind {
    table.derive_principal(ch).unwrap().kind()
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
    let mut table = SessionTable::new();
    for (o, (kind, expect_kind)) in channels.iter().zip(AC1) {
        assert_eq!(st(o, "kind"), kind, "fixture order follows AC-1");
        let expect = ob(o, "expect");
        assert_eq!(st(expect, "kind"), expect_kind, "fixture label agrees with AC-1 for {kind}");
        let ch = channel_from(&mut table, o);
        let p = table.derive_principal(&ch).unwrap();
        assert_eq!(p.kind(), PrincipalKind::parse(expect_kind).unwrap(), "{kind}");
        assert_eq!(p.id().as_str(), st(expect, "id"), "{kind}");
        assert_eq!(p.on_behalf_of().cloned(), opt_ref(expect, "on_behalf_of"), "{kind}");
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
    let mut table = SessionTable::new();
    for o in arr(&fx, "acting_for_ignored") {
        let expect = ob(o, "expect");
        let ch = channel_from(&mut table, o);
        let p = table.derive_principal(&ch).unwrap();
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
    assert_eq!(st(ob(ob(mcp, "params"), "principal"), "kind"), "human");
    // derive_principal takes the channel only, so params cannot reach it.
    let mut table = SessionTable::new();
    let ch = channel_from(&mut table, mcp);
    let p = table.derive_principal(&ch).unwrap();
    assert_eq!(p.kind(), PrincipalKind::Agent);
    assert_eq!(p.to_draft().kind, PrincipalKind::Agent);
    assert_eq!(p.to_draft().identity, CValue::Str("mcp-client:desktop-assistant".into()));
    // Every channel in the fixture carries params; none changes the labelled result
    // (checked in AC-1), and the same channel derives identically on every call.
    for o in &channels {
        let ch = channel_from(&mut table, o);
        assert_eq!(table.derive_principal(&ch).unwrap(), table.derive_principal(&ch).unwrap());
    }
}

#[test]
fn req007_self_asserted_names_never_change_the_principal() {
    let mut table = SessionTable::new();
    let mut mcp = |sa: Option<Untrusted<SelfAsserted>>| {
        let mut id = Identity::new(pid("mcp-client:x"));
        id.self_asserted = sa;
        with_identity(&mut table, ChannelKind::Mcp, "mcp-1", id)
    };
    let plain_ch = mcp(None);
    let cases = [(Some("human"), Some("human")), (Some("GUI"), None), (None, Some("system")), (Some(""), Some(""))];
    let others: Vec<(TestChannel, Option<&str>, Option<&str>)> =
        cases.iter().map(|&(c, m)| (mcp(Some(SelfAsserted::untrusted(c, m))), c, m)).collect();
    let plain = table.derive_principal(&plain_ch).unwrap();
    for (ch, client, model) in &others {
        let p = table.derive_principal(ch).unwrap();
        assert_eq!(p.kind(), plain.kind());
        assert_eq!(p.id(), plain.id());
        assert_eq!(p.on_behalf_of(), plain.on_behalf_of());
        assert_eq!(p.to_draft(), plain.to_draft());
        let sa = p.self_asserted().unwrap().as_untrusted();
        assert_eq!(sa.client_name(), *client);
        assert_eq!(sa.model_name(), *model);
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
    let mut table = SessionTable::new();
    for kind in [ChannelKind::Cli, ChannelKind::Python, ChannelKind::Control] {
        let ch = local(&mut table, kind, "s-1", "alice");
        let p = table.derive_principal(&ch).unwrap();
        assert_eq!(p.kind(), PrincipalKind::Script, "{kind}");
        assert_eq!(p.id().as_str(), "alice");
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_confirmed_upgrade_journals_then_runs_as_human() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let mut ch = cli(&mut table, "cli-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    let grant = upgrade_with(&mut table, &ch, &presence, &mut journal, &mut FakeClock::prompt_takes(4000)).unwrap();
    assert_eq!(presence.calls.get(), 1);
    assert_eq!(presence.deadline.get(), Some(Duration::from_secs(120)));
    let req = presence.seen.borrow().clone().unwrap();
    assert_eq!((req.session.as_str(), req.channel, req.os_user.as_str()), ("cli-1", ChannelKind::Cli, "alice"));
    assert_eq!(req.session_number, ch.session.number());
    assert_eq!(grant.session_number(), ch.session.number());
    // Until the channel presents the grant, it is still script.
    assert_eq!(kind_of(&table, &ch), PrincipalKind::Script);
    ch.grant = Some(grant);

    invoke_w2(&table, &ch, &mut journal).unwrap();
    assert_eq!(journal.entries.len(), 2);

    // Entry 1: principal_upgrade with session ID, method, identity commitment and time.
    let up = &journal.entries[0];
    assert_eq!(up.header.kind(), EntryKind::PrincipalUpgrade);
    assert_eq!(up.header.principal.kind, PrincipalKind::Script, "the request comes from the script session");
    let HeaderBody::PrincipalUpgrade(u) = &up.header.body else { panic!("upgrade body") };
    assert_eq!(u.session_id, format!("cli-1#{}", ch.session.number()));
    assert_eq!(u.session_id, ch.session.journal_id());
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
    assert_eq!(p.upgraded_session(), Some(ch.session.number()));

    // A later session of the same process starts as script again.
    let later = cli(&mut table, "cli-2", "alice");
    assert!(later.session.number() > ch.session.number());
    assert_eq!(kind_of(&table, &later), PrincipalKind::Script);
    // Ending the session revokes the grant even though the channel still holds it. The key
    // is consumed, so an ended key can be neither ended again nor presented again.
    assert!(table.is_upgraded(&ch.session));
    let TestChannel { session, grant, .. } = ch;
    assert!(table.end_session(session));
    assert!(table.is_empty());
    assert!(grant.is_some());
}

#[test]
fn ac4_python_session_upgrades_with_os_presence() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let mut ch = local(&mut table, ChannelKind::Python, "py-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::OsPresence);
    ch.grant = Some(upgrade(&mut table, &ch, &presence, &mut journal).unwrap());
    let HeaderBody::PrincipalUpgrade(u) = &journal.entries[0].header.body else { panic!("upgrade body") };
    assert_eq!(u.method, ConfirmMethod::OsPresence);
    assert_eq!(kind_of(&table, &ch), PrincipalKind::Human);
}

#[test]
fn guessed_or_reused_session_ids_derive_as_script() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let mut a = cli(&mut table, "cli-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    a.grant = Some(upgrade(&mut table, &a, &presence, &mut journal).unwrap());
    assert_eq!(kind_of(&table, &a), PrincipalKind::Human);

    // A second client of the same OS user names the same session label: it gets its own
    // bus-issued number and no grant, so it is script.
    let b = cli(&mut table, "cli-1", "alice");
    assert_ne!(b.session.number(), a.session.number());
    assert_eq!(b.session.label(), a.session.label());
    assert_eq!(kind_of(&table, &b), PrincipalKind::Script);
    assert!(!table.is_upgraded(&b.session));

    // Even holding a reference to A's grant, B's own key does not match it.
    let alice = a.identity.clone().unwrap();
    let forged =
        Borrowing { kind: ChannelKind::Cli, identity: alice.clone(), session: &b.session, grant: a.grant.as_ref() };
    assert_eq!(kind_of(&table, &forged), PrincipalKind::Script);
    // A's key without the grant is script.
    let keyless = Borrowing { kind: ChannelKind::Cli, identity: alice.clone(), session: &a.session, grant: None };
    assert_eq!(kind_of(&table, &keyless), PrincipalKind::Script);

    // A key and grant from another table (another process, or a restart) are worthless here.
    let mut other = SessionTable::new();
    let mut c = cli(&mut other, "cli-1", "alice");
    c.grant = Some(upgrade(&mut other, &c, &presence, &mut journal).unwrap());
    assert_eq!(kind_of(&other, &c), PrincipalKind::Human);
    assert_eq!(kind_of(&table, &c), PrincipalKind::Script);
    let mixed = Borrowing { kind: ChannelKind::Cli, identity: alice, session: &a.session, grant: c.grant.as_ref() };
    assert_eq!(kind_of(&table, &mixed), PrincipalKind::Script);
    // Upgrading with another table's key is refused before any prompt.
    let calls = presence.calls.get();
    let e = upgrade(&mut table, &c, &presence, &mut journal).unwrap_err();
    assert_eq!(code_reason(&e), "internal/invariant_violated");
    assert_eq!(presence.calls.get(), calls);
}

#[test]
fn dropping_or_replacing_a_grant_revokes_it() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let mut ch = cli(&mut table, "cli-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    ch.grant = Some(upgrade(&mut table, &ch, &presence, &mut journal).unwrap());
    assert_eq!(table.len(), 1);
    ch.grant = None; // drops the grant
    assert_eq!(table.len(), 0);
    assert!(!table.is_upgraded(&ch.session));
    assert_eq!(kind_of(&table, &ch), PrincipalKind::Script);

    // Every upgrade prompts and journals; a newer grant revokes the older one.
    let first = upgrade(&mut table, &ch, &presence, &mut journal).unwrap();
    let second = upgrade(&mut table, &ch, &presence, &mut journal).unwrap();
    assert_eq!(presence.calls.get(), 3);
    assert_eq!(journal.entries.len(), 3);
    let alice = ch.identity.clone().unwrap();
    let old = Borrowing { kind: ChannelKind::Cli, identity: alice.clone(), session: &ch.session, grant: Some(&first) };
    assert_eq!(kind_of(&table, &old), PrincipalKind::Script);
    let new = Borrowing { kind: ChannelKind::Cli, identity: alice, session: &ch.session, grant: Some(&second) };
    assert_eq!(kind_of(&table, &new), PrincipalKind::Human);
    assert_eq!(table.len(), 1);
}

#[test]
fn a_grant_counts_only_for_the_same_channel_kind_and_os_user() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let mut ch = cli(&mut table, "s-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    ch.grant = Some(upgrade(&mut table, &ch, &presence, &mut journal).unwrap());
    let alice = ch.identity.clone().unwrap();
    let mut bob = Identity::new(pid("bob"));
    bob.os_user = Some(user("bob"));
    let mut no_user = alice.clone();
    no_user.os_user = None;
    let cases = [
        (ChannelKind::Cli, bob, PrincipalKind::Script),
        (ChannelKind::Cli, no_user, PrincipalKind::Script),
        (ChannelKind::Python, alice.clone(), PrincipalKind::Script),
        (ChannelKind::Control, alice.clone(), PrincipalKind::Script),
        (ChannelKind::Mcp, alice.clone(), PrincipalKind::Agent),
        (ChannelKind::Cli, alice, PrincipalKind::Human),
    ];
    for (kind, identity, want) in cases {
        let b = Borrowing { kind, identity, session: &ch.session, grant: ch.grant.as_ref() };
        assert_eq!(kind_of(&table, &b), want, "{kind}");
    }
}

#[test]
fn req011_journal_failure_leaves_the_session_script() {
    let mut table = SessionTable::new();
    let mut journal = MemJournal { fail: true, ..MemJournal::default() };
    let ch = cli(&mut table, "s-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    let e = upgrade(&mut table, &ch, &presence, &mut journal).unwrap_err();
    assert_eq!(code_reason(&e), "internal/journal_unavailable");
    assert!(table.is_empty());
    assert_eq!(kind_of(&table, &ch), PrincipalKind::Script);
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
        let presence = FakePresence::new(outcome_from(ob(case, "outcome")));
        let mut table = SessionTable::new();
        let mut journal = MemJournal::default();
        let mut ch = cli(&mut table, "cli-5", os_user);
        let mut clock = FakeClock::prompt_takes(elapsed_ms(case));
        let got = upgrade_with(&mut table, &ch, &presence, &mut journal, &mut clock);
        assert_eq!(presence.calls.get(), 1, "{name}");
        match st(case, "expect") {
            "ok" => {
                ch.grant = Some(got.unwrap_or_else(|e| panic!("{name}: {e}")));
                assert_eq!(journal.entries.len(), 1, "{name}");
                assert_eq!(kind_of(&table, &ch), PrincipalKind::Human, "{name}");
            }
            want => {
                refused += 1;
                let e = got.expect_err(name);
                assert_eq!(e.code(), Code::UpgradeRefused, "{name}");
                assert_eq!(code_reason(&e), want, "{name}");
                assert!(journal.entries.is_empty(), "{name}: journal unchanged");
                assert!(table.is_empty(), "{name}");
                assert_eq!(kind_of(&table, &ch), PrincipalKind::Script, "{name}");
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
    let mut ch = cli(&mut table, "s-1", "alice");
    ch.identity.as_mut().unwrap().os_user = None;
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    let e = upgrade(&mut table, &ch, &presence, &mut journal).unwrap_err();
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
        let mut table = SessionTable::new();
        let ch = channel_from(&mut table, o);
        if ch.kind.upgradable() {
            continue;
        }
        let mut journal = MemJournal::default();
        let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
        let e = upgrade(&mut table, &ch, &presence, &mut journal).expect_err(ch.kind.as_str());
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
        let mut table = SessionTable::new();
        let ch = channel_from(&mut table, o);
        let mut journal = MemJournal::default();
        let e = table.derive_principal(&ch).expect_err(name);
        assert_eq!(code_reason(&e), "unauthenticated/no_identity", "{name}");
        let e = invoke_w2(&table, &ch, &mut journal).expect_err(name);
        assert_eq!(e.code(), Code::Unauthenticated, "{name}");
        assert!(journal.entries.is_empty(), "{name}: journals nothing");
        let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
        let e = upgrade(&mut table, &ch, &presence, &mut journal).expect_err(name);
        assert_eq!(e.code(), Code::Unauthenticated, "{name}");
        assert_eq!(presence.calls.get(), 0, "{name}");
        assert!(journal.entries.is_empty(), "{name}");
    }
    let err = BusError::Unauthenticated(UnauthenticatedReason::NoIdentity, Detail::empty());
    let json = String::from_utf8(err.to_json().unwrap()).unwrap();
    assert_eq!(json, r#"{"code":"unauthenticated","detail":"","reason":"no_identity"}"#);
}

// ------------------------------------------------------------------ AC-8 and attribution

fn recompute_for(table: &mut SessionTable, trigger: PrincipalRef) -> TestChannel {
    let mut id = Identity::new(pid("core:recompute"));
    id.acting_for = Some(trigger);
    with_identity(table, ChannelKind::Internal, "core-1", id)
}

#[test]
fn ac8_internal_recompute_names_the_triggering_human() {
    let mut table = SessionTable::new();
    let gui = local(&mut table, ChannelKind::Gui, "desk-1", "alice");
    let human = table.derive_principal(&gui).unwrap();
    assert_eq!(human.kind(), PrincipalKind::Human);
    let internal = recompute_for(&mut table, human.reference());
    let p = table.derive_principal(&internal).unwrap();
    assert_eq!(p.kind(), PrincipalKind::System);
    let behalf = p.on_behalf_of().unwrap();
    assert_eq!((behalf.kind, behalf.id.as_str()), (PrincipalKind::Human, "alice"));
    let mut want = BTreeMap::new();
    want.insert("id".to_string(), CValue::Str("alice".into()));
    want.insert("kind".to_string(), CValue::Str("human".into()));
    assert_eq!(p.to_draft().on_behalf_of, Some(CValue::Obj(want)));
}

#[test]
fn on_behalf_of_keeps_the_trigger_kind() {
    // alice as script (CLI) and alice as human (GUI) trigger the same recompute.
    let mut table = SessionTable::new();
    let mut journal = MemJournal::default();
    let cli_ch = local(&mut table, ChannelKind::Cli, "cli-1", "alice");
    let gui_ch = local(&mut table, ChannelKind::Gui, "desk-1", "alice");
    let triggers = [table.derive_principal(&cli_ch).unwrap(), table.derive_principal(&gui_ch).unwrap()];
    assert_eq!(triggers[0].id(), triggers[1].id());
    let mut drafts = Vec::new();
    for t in &triggers {
        let ch = recompute_for(&mut table, t.reference());
        let p = table.derive_principal(&ch).unwrap();
        assert_eq!(p.on_behalf_of().map(|r| r.kind), Some(t.kind()));
        drafts.push(p.to_draft().on_behalf_of.unwrap());
        invoke_w2(&table, &ch, &mut journal).unwrap();
    }
    assert_ne!(drafts[0], drafts[1], "alice-script and alice-human are distinguishable");
    // The journal commits the object: each verifies against its own value only.
    for (entry, value) in journal.entries.iter().zip(&drafts) {
        let salt = entry.payload.salts.get(&Field::OnBehalfOf);
        verify_commitment(&entry.header, Field::OnBehalfOf, salt, Some(value)).unwrap();
    }
    let salt = journal.entries[0].payload.salts.get(&Field::OnBehalfOf);
    assert!(verify_commitment(&journal.entries[0].header, Field::OnBehalfOf, salt, Some(&drafts[1])).is_err());
}

// ------------------------------------------------------------------ bounds

#[test]
fn identifiers_follow_the_labelled_cases() {
    let fx = fixture("ids.json");
    assert_eq!(st(&fx, "license"), "CC-BY-4.0");
    let cases = arr(&fx, "cases");
    assert_eq!(cases.len(), 44);
    // Every character outside printable ASCII is written as a \uXXXX escape, as the note says.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/bus/principal/ids.json");
    let raw = std::fs::read(path).unwrap();
    assert!(raw.iter().all(|b| (0x20..0x7f).contains(b) || *b == b'\n'), "ids.json is printable ASCII");
    for case in cases {
        let name = st(case, "name");
        let text = st(case, "text");
        let want = st(case, "expect");
        let got = [
            PrincipalId::new(text).map(|_| ()),
            ClientId::new(text).map(|_| ()),
            OsUser::new(text).map(|_| ()),
            SessionId::new(text).map(|_| ()),
        ];
        for g in got {
            let code = match g {
                Ok(()) => "ok",
                Err(e) => e.code(),
            };
            assert_eq!(code, want, "{name}");
        }
    }
}

#[test]
fn identifiers_are_bounded_and_case_significant() {
    assert_eq!(PrincipalId::new(""), Err(IdError::Empty));
    assert_eq!(OsUser::new(&"a".repeat(MAX_ID_BYTES + 1)), Err(IdError::TooLong));
    assert!(SessionId::new(&"a".repeat(MAX_ID_BYTES)).is_ok());
    // 128 two-byte characters are 256 bytes; one more is too long.
    assert!(OsUser::new(&"\u{e9}".repeat(128)).is_ok());
    assert_eq!(OsUser::new(&"\u{e9}".repeat(129)), Err(IdError::TooLong));
    assert_eq!(IdError::NotNfc.code(), "not_nfc");
    // OS user names compare byte for byte: a case-only difference is another user. The
    // PLAT provider must report the canonical spelling on case-insensitive platforms.
    assert_ne!(user("Alice"), user("alice"));
    assert_eq!(PrincipalId::from(&user("alice")).as_str(), "alice");
}

#[test]
fn session_numbers_are_monotonic_and_never_reused() {
    let mut table = SessionTable::new();
    let mut last = 0;
    for _ in 0..100 {
        let k = table.open_session(sid("same-label")).unwrap();
        assert!(k.number() > last);
        last = k.number();
        assert!(!table.end_session(k));
    }
}

#[test]
fn session_table_is_bounded() {
    let mut table = SessionTable::new();
    let mut journal = CountingJournal::default();
    let presence = FakePresence::confirms("alice", ConfirmMethod::OsPresence);
    let mut held = Vec::new();
    for i in 0..MAX_UPGRADED_SESSIONS {
        let mut ch = cli(&mut table, &format!("s-{i}"), "alice");
        ch.grant = Some(upgrade(&mut table, &ch, &presence, &mut journal).unwrap());
        held.push(ch);
    }
    assert_eq!(table.len(), MAX_UPGRADED_SESSIONS);
    let calls = presence.calls.get();
    let extra = cli(&mut table, "one-more", "alice");
    let e = upgrade(&mut table, &extra, &presence, &mut journal).unwrap_err();
    assert_eq!(code_reason(&e), "internal/storage");
    assert_eq!(presence.calls.get(), calls, "no prompt when the table is full");
    assert_eq!(journal.0, MAX_UPGRADED_SESSIONS);
    // Closing a session frees a row.
    let first = held.remove(0);
    assert!(table.end_session(first.session));
    upgrade(&mut table, &extra, &presence, &mut journal).unwrap();
}

#[test]
fn unicode_tables_are_sorted_and_pinned_to_16_0() {
    let tables = [FORMAT_RANGES, UNASSIGNED_RANGES, DEFAULT_IGNORABLE_RANGES, PRIVATE_USE_RANGES, BLANK_CHARS];
    for t in tables {
        assert!(!t.is_empty());
        for &(lo, hi) in t {
            assert!(lo <= hi && hi <= 0x10_FFFF);
        }
        for w in t.windows(2) {
            assert!(w[0].1 < w[1].0, "sorted and non-overlapping");
        }
    }
    let has = |t: &[(u32, u32)], u: u32| t.iter().any(|&(lo, hi)| lo <= u && u <= hi);
    // Hand-checked against the Unicode 16.0 character database.
    assert!(!has(UNASSIGNED_RANGES, 0x1C89), "U+1C89 CYRILLIC CAPITAL LETTER TJE is new in 16.0");
    assert!(!has(UNASSIGNED_RANGES, 0x0897), "U+0897 ARABIC PEPET is new in 16.0");
    assert!(has(UNASSIGNED_RANGES, 0x1C8B));
    assert!(has(UNASSIGNED_RANGES, 0xFFFE) && has(UNASSIGNED_RANGES, 0x10_FFFF), "noncharacters are Cn");
    assert!(!has(UNASSIGNED_RANGES, 0xE000) && !has(UNASSIGNED_RANGES, 0xD7A3));
    for u in [0x034F, 0x115F, 0x1160, 0x17B4, 0x180F, 0x2065, 0x3164, 0xFE00, 0xFFA0, 0xE0000, 0xE0FFF] {
        assert!(has(DEFAULT_IGNORABLE_RANGES, u), "U+{u:04X} is Default_Ignorable_Code_Point");
    }
    for u in [0x0020, 0x00A0, 0x0600, 0xFFF9, 0x13430, 0x2800] {
        assert!(!has(DEFAULT_IGNORABLE_RANGES, u), "U+{u:04X} is not Default_Ignorable_Code_Point");
    }
    // Every Zs character of Unicode 16.0 except U+0020, plus U+2800, is a blank.
    let zs = [0x00A0, 0x1680, 0x2000, 0x2005, 0x200A, 0x202F, 0x205F, 0x3000, 0x2800];
    for u in zs {
        assert!(has(BLANK_CHARS, u), "U+{u:04X}");
    }
    assert!(!has(BLANK_CHARS, 0x0020) && !has(BLANK_CHARS, 0x200B));
    assert_eq!(IdError::BlankChar.code(), "blank_char");
    assert_eq!(IdError::DefaultIgnorable.code(), "default_ignorable");
    assert_eq!(IdError::Unassigned.code(), "unassigned");
}

// ------------------------------------------------------------------ upgrade without a held borrow

/// A presence provider that, while the person is being asked, runs `hook` against the
/// shared table. That compiles and runs only because no table borrow is held during the
/// prompt.
type Hook<'a> = Box<dyn FnOnce(&mut SessionTable) + 'a>;

struct DuringPrompt<'a> {
    table: &'a RefCell<SessionTable>,
    hook: RefCell<Option<Hook<'a>>>,
    answer: FakePresence,
}

impl<'a> DuringPrompt<'a> {
    fn new(table: &'a RefCell<SessionTable>, hook: impl FnOnce(&mut SessionTable) + 'a) -> Self {
        let answer = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
        DuringPrompt { table, hook: RefCell::new(Some(Box::new(hook))), answer }
    }
}

impl HumanPresence for DuringPrompt<'_> {
    fn confirm(&self, req: &UpgradeRequest, deadline: Duration) -> PresenceOutcome {
        let hook = self.hook.borrow_mut().take();
        if let Some(hook) = hook {
            hook(&mut self.table.borrow_mut());
        }
        self.answer.confirm(req, deadline)
    }
}

#[test]
fn the_table_stays_usable_while_a_person_is_asked() {
    let table = RefCell::new(SessionTable::new());
    let mut journal = MemJournal::default();
    let mut ch = cli(&mut table.borrow_mut(), "cli-1", "alice");
    let opened = Cell::new(0);
    let pending = table.borrow().prepare_upgrade(&ch).unwrap();
    assert_eq!(pending.request().session_number, ch.session.number());
    let presence = DuringPrompt::new(&table, |t| {
        // Other connections open and close while the prompt is up.
        let k = t.open_session(sid("cli-2")).unwrap();
        opened.set(k.number());
        assert!(!t.end_session(k));
    });
    let confirmed = pending.confirm(&presence, &mut FakeClock::prompt_takes(30_000)).unwrap();
    assert_eq!(presence.answer.calls.get(), 1);
    drop(presence);
    assert!(opened.get() > ch.session.number());
    assert_eq!(confirmed.session_number(), ch.session.number());
    ch.grant = Some(table.borrow_mut().commit_upgrade(confirmed, &mut journal, &ctx()).unwrap());
    assert_eq!(journal.entries.len(), 1);
    assert_eq!(kind_of(&table.borrow(), &ch), PrincipalKind::Human);
}

#[test]
fn a_session_ended_mid_prompt_is_not_upgraded() {
    let table = RefCell::new(SessionTable::new());
    let mut journal = MemJournal::default();
    let ch = cli(&mut table.borrow_mut(), "cli-1", "alice");
    let pending = table.borrow().prepare_upgrade(&ch).unwrap();
    let TestChannel { session, .. } = ch;
    // The channel closes while the person is being asked.
    let presence = DuringPrompt::new(&table, move |t| assert!(!t.end_session(session)));
    let confirmed = pending.confirm(&presence, &mut FakeClock::prompt_takes(1)).unwrap();
    assert_eq!(presence.answer.calls.get(), 1, "the person did confirm");
    let e = table.borrow_mut().commit_upgrade(confirmed, &mut journal, &ctx()).unwrap_err();
    assert_eq!(code_reason(&e), "conflict/head_moved");
    assert!(journal.entries.is_empty(), "journal unchanged");
    assert!(table.borrow().is_empty(), "no grant");
}

#[test]
fn a_session_whose_key_is_dropped_mid_prompt_is_not_upgraded() {
    let table = RefCell::new(SessionTable::new());
    let mut journal = MemJournal::default();
    let ch = cli(&mut table.borrow_mut(), "cli-1", "alice");
    let pending = table.borrow().prepare_upgrade(&ch).unwrap();
    let presence = DuringPrompt::new(&table, move |_| drop(ch));
    let confirmed = pending.confirm(&presence, &mut FakeClock::prompt_takes(1)).unwrap();
    let e = table.borrow_mut().commit_upgrade(confirmed, &mut journal, &ctx()).unwrap_err();
    assert_eq!(code_reason(&e), "conflict/head_moved");
    assert!(journal.entries.is_empty());
    assert!(table.borrow().is_empty());
}

#[test]
fn a_competing_upgrade_mid_prompt_leaves_the_slower_one_uncommitted() {
    let table = RefCell::new(SessionTable::new());
    let journal = RefCell::new(MemJournal::default());
    let mut ch = cli(&mut table.borrow_mut(), "cli-1", "alice");
    let won: RefCell<Option<UpgradeGrant>> = RefCell::new(None);
    let pending = table.borrow().prepare_upgrade(&ch).unwrap();
    {
        let quick = FakePresence::confirms("alice", ConfirmMethod::OsPresence);
        // A second request for the same session is prepared, confirmed and committed while
        // the first one's person is still being asked.
        let presence = DuringPrompt::new(&table, |t| {
            let g = upgrade(t, &ch, &quick, &mut *journal.borrow_mut()).unwrap();
            *won.borrow_mut() = Some(g);
        });
        let confirmed = pending.confirm(&presence, &mut FakeClock::prompt_takes(1)).unwrap();
        let e = table.borrow_mut().commit_upgrade(confirmed, &mut *journal.borrow_mut(), &ctx()).unwrap_err();
        assert_eq!(code_reason(&e), "conflict/head_moved");
        assert_eq!(quick.calls.get(), 1);
    }
    // Only the competing upgrade was journalled and granted.
    let j = journal.borrow();
    assert_eq!(j.entries.len(), 1);
    let HeaderBody::PrincipalUpgrade(u) = &j.entries[0].header.body else { panic!("upgrade body") };
    assert_eq!(u.method, ConfirmMethod::OsPresence);
    ch.grant = won.into_inner();
    assert_eq!(kind_of(&table.borrow(), &ch), PrincipalKind::Human);
    assert_eq!(table.borrow().len(), 1);
}

#[test]
fn a_confirmation_commits_only_in_the_table_that_prepared_it() {
    let mut a = SessionTable::new();
    let mut b = SessionTable::new();
    let mut journal = MemJournal::default();
    let ch = cli(&mut a, "cli-1", "alice");
    let presence = FakePresence::confirms("alice", ConfirmMethod::GuiPrompt);
    let confirmed = a.prepare_upgrade(&ch).unwrap().confirm(&presence, &mut FakeClock::prompt_takes(1)).unwrap();
    let e = b.commit_upgrade(confirmed, &mut journal, &ctx()).unwrap_err();
    assert_eq!(code_reason(&e), "internal/invariant_violated");
    assert!(journal.entries.is_empty());
    assert!(a.is_empty() && b.is_empty());
}

#[test]
fn the_table_is_rechecked_for_room_at_commit() {
    let mut table = SessionTable::new();
    let mut journal = CountingJournal::default();
    let presence = FakePresence::confirms("alice", ConfirmMethod::OsPresence);
    let late = cli(&mut table, "late", "alice");
    // Prepared while the table still has room...
    let pending = table.prepare_upgrade(&late).unwrap();
    let mut held = Vec::new();
    for i in 0..MAX_UPGRADED_SESSIONS {
        let mut ch = cli(&mut table, &format!("s-{i}"), "alice");
        ch.grant = Some(upgrade(&mut table, &ch, &presence, &mut journal).unwrap());
        held.push(ch);
    }
    // ...but it filled up during the prompt.
    let confirmed = pending.confirm(&presence, &mut FakeClock::prompt_takes(1)).unwrap();
    let e = table.commit_upgrade(confirmed, &mut journal, &ctx()).unwrap_err();
    assert_eq!(code_reason(&e), "internal/storage");
    assert_eq!(journal.0, MAX_UPGRADED_SESSIONS);
    assert!(!table.is_upgraded(&late.session));
}
