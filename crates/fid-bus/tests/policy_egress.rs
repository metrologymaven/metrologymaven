//! Acceptance criteria of spec node CORE.BUS.POL.EGRESS (AC-1 in part, AC-2, AC-3; AC-5 is the
//! `compile_fail` doctests of `fid-net`), against `fixtures/core/bus/pol/egress/artifact.bin`
//! and `cases.json`, written by the independent reference
//! `fixtures/core/bus/pol/egress/make_fixtures.py` (digests computed with Python's hashlib,
//! grammar cases hand-labelled from the spec).
//!
//! No test opens a connection to another host: `https` sends go to an in-process recording
//! transport, and `network_root` sends write into a temporary folder.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code};
use fid_bus::handler::{CancelSource, CancelToken, Event, HandlerCtx, ValidParams};
use fid_bus::policy::egress::{
    authorise, check_root_path, command_defs, issue_permit_for_tests, ApprovalDialog, Destination, DestinationId,
    Destinations, EgressApproval, EgressDecision, EgressPrompt, EgressSender, HttpsUrl, NetworkRootTransport, Outcome,
    PermitScope, SendOutcome, Sent, Transport, Transports, APPROVAL_MAX_AGE, DESTINATIONS_KEY, EGRESS_EVENT,
    MAX_NOTE_CHARS, MAX_RECEIPT_CHARS, SEND_COMMAND,
};
use fid_bus::policy::projection::{EntryOrigin, RecordContext, RequestBook, RequestId, RequestState};
use fid_bus::principal::{
    Channel, ChannelKind, ConfirmMethod, Identity, JournalAppend, OsUser, PresenceOutcome, PrincipalId, PrincipalKind,
    PrincipalRef, SessionId, SessionKey, SessionTable, UpgradeClock,
};
use fid_bus::Untrusted;
use fid_canon::{parse, CValue, Digest as CanonDigest};
use fid_journal::{DraftBody, EntryDraft, EntryTime, Manifests};
use fid_registry::CommandDef;
use fid_store::cas::{BlobStore, PutOpts};
use fid_store::settings::check::validate_value;
use fid_store::settings::{FileStore, Scope, Sources};
use fid_types::{CommandId, Digest};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

type Obj = BTreeMap<String, CValue>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(rel: &str) -> Vec<u8> {
    std::fs::read(root().join("fixtures/core/bus/pol/egress").join(rel)).unwrap()
}

fn cases() -> CValue {
    parse(&fixture("cases.json")).unwrap()
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

fn boolean(o: &Obj, k: &str) -> bool {
    match o.get(k) {
        Some(CValue::Bool(b)) => *b,
        other => panic!("{k}: {other:?}"),
    }
}

fn json(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

fn assert_err<T: std::fmt::Debug>(r: Result<T, BusError>, code: Code, reason: &str) {
    let e = r.unwrap_err();
    assert_eq!((e.code(), e.reason()), (code, reason), "{e}");
    assert_ne!(e.code(), Code::Internal);
}

fn artifact_case() -> (Vec<u8>, Digest, u64) {
    let c = cases();
    let a = obj(obj(&c).get("artifact").unwrap());
    let size = match a.get("size") {
        Some(CValue::Num(n)) => *n as u64,
        other => panic!("size: {other:?}"),
    };
    (fixture(st(a, "file")), Digest::parse(st(a, "digest")).unwrap(), size)
}

// ------------------------------------------------------------------ channels

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

// ------------------------------------------------------------------ dialog, clock, journal

/// An approval dialog that answers with a fixed outcome and records what it showed.
struct Dialog {
    answer: PresenceOutcome,
    shown: RefCell<Vec<EgressPrompt>>,
}

impl Dialog {
    fn confirm_as(user: &str) -> Dialog {
        Dialog {
            answer: PresenceOutcome::Confirmed {
                os_user: OsUser::new(user).unwrap(),
                method: ConfirmMethod::GuiPrompt,
            },
            shown: RefCell::new(Vec::new()),
        }
    }

    fn answering(answer: PresenceOutcome) -> Dialog {
        Dialog { answer, shown: RefCell::new(Vec::new()) }
    }
}

impl ApprovalDialog for Dialog {
    fn confirm(&self, prompt: &EgressPrompt, deadline: Duration) -> PresenceOutcome {
        assert!(deadline > Duration::ZERO);
        self.shown.borrow_mut().push(prompt.clone());
        self.answer.clone()
    }
}

struct Clock;

impl UpgradeClock for Clock {
    fn now(&mut self) -> EntryTime {
        time()
    }
}

fn time() -> EntryTime {
    EntryTime { utc: "2026-11-20T10:00:00Z".into(), mono_ns: 1_000, sync: "ntp".into() }
}

/// `seconds` after [`time`], by the monotonic clock.
fn later(seconds: u64) -> EntryTime {
    EntryTime { utc: "2026-11-20T10:00:00Z".into(), mono_ns: 1_000 + seconds * 1_000_000_000, sync: "ntp".into() }
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

fn record_ctx() -> RecordContext {
    RecordContext {
        stream: "record".into(),
        branch: "main".into(),
        manifests: Manifests { registry: CanonDigest([0x11; 32]), kernel: CanonDigest([0x22; 32]) },
    }
}

const HEAD: CanonDigest = CanonDigest([0x77; 32]);

fn rid(n: u8) -> RequestId {
    RequestId::parse(&format!("0193a5b0-1c2d-7e3f-8a4b-0000000000{n:02x}")).unwrap()
}

// ------------------------------------------------------------------ transports

/// An in-process stand-in for an `https` endpoint: records every send and answers with a
/// fixed outcome.
struct Recorder {
    sends: Mutex<Vec<(String, Vec<u8>)>>,
    answer: SendOutcome,
}

impl Recorder {
    fn new(answer: SendOutcome) -> Recorder {
        Recorder { sends: Mutex::new(Vec::new()), answer }
    }

    fn ok() -> Recorder {
        Recorder::new(SendOutcome::Sent { receipt: Some(Untrusted::new("stored as 42".into())) })
    }

    fn count(&self) -> usize {
        self.sends.lock().unwrap().len()
    }

    fn received(&self) -> Vec<(String, Vec<u8>)> {
        self.sends.lock().unwrap().clone()
    }
}

impl Transport for Recorder {
    fn send(
        &self,
        permit: &fid_bus::NetPermit<'_>,
        destination: &Destination,
        _artifact: &Digest,
        bytes: &[u8],
        _cancel: &CancelToken,
    ) -> SendOutcome {
        assert_eq!(permit.class(), fid_types::SideEffect::X1);
        self.sends.lock().unwrap().push((destination.target(), bytes.to_vec()));
        self.answer.clone()
    }
}

// ------------------------------------------------------------------ world

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A temporary folder, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(what: &str) -> Temp {
        let p = std::env::temp_dir().join(format!(
            "fid-bus-egress-{what}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const UPLOAD_URL: &str = "https://upload.example.org/inbox";

fn destinations_value(share: &Path, url: &str) -> CValue {
    let mut lab = Obj::new();
    lab.insert("id".into(), CValue::Str("lab-share".into()));
    lab.insert("kind".into(), CValue::Str("network_root".into()));
    lab.insert("path".into(), CValue::Str(share.to_string_lossy().into_owned()));
    let mut up = Obj::new();
    up.insert("id".into(), CValue::Str("upload".into()));
    up.insert("kind".into(), CValue::Str("https".into()));
    up.insert("url".into(), CValue::Str(url.into()));
    up.insert("method".into(), CValue::Str("PUT".into()));
    CValue::Arr(vec![CValue::Obj(lab), CValue::Obj(up)])
}

struct World {
    table: SessionTable,
    book: RequestBook,
    journal: Journal,
    _store_dir: Temp,
    share: Temp,
    store: BlobStore,
    destinations: Destinations,
    digest: Digest,
    bytes: Vec<u8>,
    /// The time `authorise` runs at: one minute after the dialog by default.
    now: EntryTime,
}

impl World {
    fn new() -> World {
        let store_dir = Temp::new("store");
        let share = Temp::new("share");
        let store = BlobStore::open(&store_dir.0).unwrap();
        let (bytes, digest, size) = artifact_case();
        let addr = store.put_bytes(&bytes, PutOpts::default()).unwrap();
        assert_eq!((addr.digest(), addr.size()), (digest, size), "the store agrees with the Python digest");
        let destinations = Destinations::from_value(&destinations_value(&share.0, UPLOAD_URL)).unwrap();
        World {
            table: SessionTable::new(),
            book: RequestBook::new(),
            journal: Journal::default(),
            _store_dir: store_dir,
            share,
            store,
            destinations,
            digest,
            bytes,
            now: later(60),
        }
    }

    fn channel(&mut self, kind: ChannelKind, subject: &str) -> TestChannel {
        channel(&mut self.table, kind, subject)
    }

    fn dest(&self, id: &str) -> &Destination {
        self.destinations.lookup(id).unwrap()
    }

    fn approval(&self, ch: &TestChannel, digest: Digest, dest: &str) -> EgressApproval {
        let dialog = Dialog::confirm_as(ch.identity.subject.as_str());
        EgressApproval::ask(&self.table, ch, &dialog, &mut Clock, digest, self.dest(dest)).unwrap()
    }

    fn authorise(
        &self,
        ch: &TestChannel,
        p: &CValue,
        approval_of: Option<&RequestId>,
        consent: Option<EgressApproval>,
    ) -> Result<EgressDecision, BusError> {
        let view = self.book.view(HEAD);
        authorise(
            &self.table,
            ch,
            &def(),
            &ValidParams::for_tests(p.clone()),
            approval_of,
            consent,
            &self.destinations,
            &view,
            &self.now,
        )
        .map(|(_, d)| d)
    }

    fn send(
        &self,
        ch: &TestChannel,
        decision: EgressDecision,
        transports: Transports<'_>,
        cancel: CancelToken,
    ) -> Result<Sent, BusError> {
        let EgressDecision::Send(cleared) = decision else { panic!("not cleared to send") };
        let d = def();
        let mut scope = PermitScope::new();
        let permit = issue_permit_for_tests(&d, &mut scope).unwrap();
        let ctx = HandlerCtx::derive(&self.table, ch, 0, cancel).unwrap();
        EgressSender::new(&self.store, transports).send(*cleared, &self.destinations, &permit, &ctx)
    }
}

fn def() -> CommandDef {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    defs.into_iter().next().unwrap()
}

fn params(digest: &Digest, dest: &str) -> CValue {
    json(&format!(r#"{{"artifact":"{digest}","destination":"{dest}"}}"#))
}

fn https<'a>(r: &'a Recorder) -> Transports<'a> {
    Transports { network_root: None, https: Some(r) }
}

fn event_body(sent: &Sent) -> &Obj {
    assert_eq!(sent.events.len(), 1);
    assert_eq!(sent.events[0].type_id().as_str(), EGRESS_EVENT);
    obj(sent.events[0].body())
}

fn num(o: &Obj, k: &str) -> f64 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n,
        other => panic!("{k}: {other:?}"),
    }
}

/// The command-schema check: real parameters and real results against the definition.
fn check_schemas(p: &CValue, sent: &Sent) {
    let d = def();
    validate_value(&d.params_schema, p).unwrap();
    validate_value(&d.result_schema, &sent.result).unwrap();
    // `pattern` is not a structural keyword of the validator; check it here.
    let artifact = st(obj(p), "artifact");
    assert!(Digest::is_valid(artifact));
    assert!(DestinationId::new(st(obj(p), "destination")).is_some());
}

// ------------------------------------------------------------------ fixtures and grammar

#[test]
fn the_artifact_fixture_matches_its_python_digest() {
    let (bytes, digest, size) = artifact_case();
    assert_eq!(bytes.len() as u64, size);
    assert_eq!(Digest::from_bytes(fid_canon::digest_bytes(&bytes).0), digest);
}

#[test]
fn destination_ids_follow_the_grammar() {
    let c = cases();
    let ids = arr(obj(&c), "ids");
    assert!(ids.len() >= 10);
    for case in ids {
        let o = obj(case);
        let got = DestinationId::new(st(o, "id"));
        assert_eq!(got.is_some(), boolean(o, "valid"), "{}", st(o, "name"));
        if let Some(id) = got {
            assert_eq!(id.as_str(), st(o, "id"), "an accepted ID is kept exactly");
        }
    }
}

#[test]
fn https_urls_follow_the_strict_form() {
    let c = cases();
    for case in arr(obj(&c), "urls") {
        let o = obj(case);
        let got = HttpsUrl::parse(st(o, "url"));
        assert_eq!(got.is_ok(), boolean(o, "valid"), "{}: {:?}", st(o, "name"), got);
        if let Ok(u) = got {
            assert_eq!(u.as_str(), st(o, "url"), "nothing is normalised");
        }
    }
    let u = HttpsUrl::parse("https://upload.example.org:8443/inbox/").unwrap();
    assert_eq!((u.host(), u.port(), u.path()), ("upload.example.org", 8443, "/inbox/"));
    assert_eq!(HttpsUrl::parse("https://a.example/x").unwrap().port(), 443);
}

fn applies(platform: &str) -> bool {
    match platform {
        "all" => true,
        "unix" => cfg!(unix),
        "windows" => cfg!(windows),
        other => panic!("platform {other}"),
    }
}

#[test]
fn destination_settings_are_refused_whole() {
    let c = cases();
    let mut checked = 0;
    for case in arr(obj(&c), "settings") {
        let o = obj(case);
        if !applies(st(o, "platform")) {
            continue;
        }
        let got = Destinations::from_value(o.get("value").unwrap());
        assert_eq!(got.is_ok(), boolean(o, "valid"), "{}: {:?}", st(o, "name"), got);
        checked += 1;
    }
    assert!(checked >= 15);
}

#[test]
fn configuration_digests_match_python() {
    let c = cases();
    let mut checked = 0;
    for case in arr(obj(&c), "config_digests") {
        let o = obj(case);
        let d = o.get("destination").unwrap();
        let path_ok = match obj(d).get("path") {
            Some(CValue::Str(p)) => Path::new(p).is_absolute(),
            _ => true,
        };
        if !path_ok {
            continue;
        }
        let set = Destinations::from_value(&CValue::Arr(vec![d.clone()])).unwrap();
        let got = set.lookup(st(obj(d), "id")).unwrap();
        assert_eq!(got.digest().as_str(), st(o, "digest"));
        checked += 1;
    }
    assert_eq!(checked, 2, "the https case and the root case of this platform");
}

#[test]
fn the_definition_is_x1_request_only_and_observed() {
    let d = def();
    assert_eq!((d.id.as_str(), d.major), (SEND_COMMAND, 1));
    assert_eq!(d.side_effect, fid_types::SideEffect::X1);
    assert_eq!(d.determinism, fid_types::Determinism::N);
    assert_eq!(d.mcp_projection, fid_registry::Projection::RequestOnly);
    assert_eq!(fid_bus::policy::projection::mcp_projection(&d), fid_registry::Projection::RequestOnly);
    assert!(!d.human_only);
    assert_eq!(d.effects.len(), 1);
}

// ------------------------------------------------------------------ AC-1

/// AC-1 (the bus part): a GUI human confirms the dialog; the endpoint receives exactly the
/// bytes whose SHA-256 is the digest, once; the entry records digest, size, destination and
/// `sent`. The endpoint is the in-process recorder (the TLS transport is not in this build).
#[test]
fn ac1_a_gui_human_with_the_dialog_confirmed_sends_the_exact_bytes_once() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let dialog = Dialog::confirm_as("alice");
    let consent = EgressApproval::ask(&w.table, &alice, &dialog, &mut Clock, w.digest, w.dest("upload")).unwrap();
    let shown = dialog.shown.borrow().clone();
    assert_eq!(shown.len(), 1);
    assert_eq!((shown[0].artifact, shown[0].destination.as_str()), (w.digest, "upload"));
    assert_eq!((shown[0].kind, shown[0].target.as_str(), shown[0].method), ("https", UPLOAD_URL, Some("PUT")));
    assert_eq!(consent.approver(), &PrincipalRef { kind: PrincipalKind::Human, id: pid("alice") });

    let p = params(&w.digest, "upload");
    let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
    let r = Recorder::ok();
    let sent = w.send(&alice, decision, https(&r), CancelToken::never()).unwrap();

    let got = r.received();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, UPLOAD_URL);
    assert_eq!(got[0].1, w.bytes);
    assert_eq!(Digest::from_bytes(fid_canon::digest_bytes(&got[0].1).0), w.digest);

    assert_eq!(sent.outcome, Outcome::Sent);
    assert!(sent.request_approval.is_none());
    let b = event_body(&sent);
    assert_eq!(st(b, "artifact"), w.digest.as_str());
    assert_eq!(num(b, "bytes"), w.bytes.len() as f64);
    assert_eq!(st(b, "destination"), "upload");
    assert_eq!(st(b, "destination_digest"), w.dest("upload").digest().as_str());
    assert_eq!(st(b, "outcome"), "sent");
    assert_eq!(st(b, "remote_receipt"), "stored as 42");
    assert!(!boolean(b, "already_present"));
    let approval = obj(b.get("approval").unwrap());
    assert_eq!(st(approval, "at"), "2026-11-20T10:00:00Z");
    assert_eq!(st(approval, "method"), "gui_prompt");
    assert_eq!(st(obj(approval.get("approved_by").unwrap()), "kind"), "human");
    assert_eq!(st(obj(&sent.result), "outcome"), "sent");
    check_schemas(&p, &sent);
}

/// AC-1 with a `network_root` destination: the file in the share holds exactly the bytes;
/// a second send never rewrites it and, the content being the same, is recorded as `sent`.
#[test]
fn ac1_a_network_root_send_writes_a_new_file_and_never_overwrites() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "lab-share");
    let t = NetworkRootTransport::new();
    let transports = Transports { network_root: Some(&t), https: None };

    let consent = w.approval(&alice, w.digest, "lab-share");
    let sent = w.send(&alice, w.authorise(&alice, &p, None, Some(consent)).unwrap(), transports, CancelToken::never());
    let sent = sent.unwrap();
    assert_eq!(sent.outcome, Outcome::Sent);
    assert!(!boolean(event_body(&sent), "already_present"), "a new file was written");
    let file = w.share.0.join(w.digest.hex());
    assert_eq!(std::fs::read(&file).unwrap(), w.bytes);
    check_schemas(&p, &sent);

    let consent = w.approval(&alice, w.digest, "lab-share");
    let again = w.send(&alice, w.authorise(&alice, &p, None, Some(consent)).unwrap(), transports, CancelToken::never());
    let again = again.unwrap();
    // Maintainer decision on #336: an existing file with the same content counts as sent,
    // with the distinct flag that nothing was written (#336 review).
    assert_eq!(again.outcome, Outcome::Sent);
    assert!(boolean(event_body(&again), "already_present"));
    assert!(event_body(&again).get("remote_receipt").is_none());
    assert_eq!(std::fs::read(&file).unwrap(), w.bytes);
    assert_eq!(std::fs::read_dir(&w.share.0).unwrap().count(), 1, "no partial file is left");
    check_schemas(&p, &again);
}

// ------------------------------------------------------------------ AC-2

/// AC-2: an MCP agent's `egress_send` files a pending request and nothing is sent; a human
/// approving it (request approval plus the dialog) sends the bytes once; the request cannot
/// be approved again.
#[test]
fn ac2_an_agent_files_a_request_and_a_human_approval_sends_once() {
    let mut w = World::new();
    let agent = w.channel(ChannelKind::Mcp, "claude-desktop");
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "upload");
    let r = Recorder::ok();

    let EgressDecision::File(filing) = w.authorise(&agent, &p, None, None).unwrap() else { panic!("agent sent") };
    let view = w.book.view(HEAD);
    let id = filing.commit(rid(1), &view, time(), &record_ctx(), &mut w.journal).unwrap();
    let draft = w.journal.drafts.last().unwrap().clone();
    let DraftBody::Command(c) = &draft.body else { panic!("not a command entry") };
    let events: Vec<Event> = c.events.iter().map(|e| Event::from_value(e.clone()).unwrap()).collect();
    w.book.apply_entry(&EntryOrigin::from_draft(&draft).unwrap(), &events).unwrap();
    assert_eq!(w.book.get(&id).unwrap().state(), RequestState::Pending);
    assert_eq!(r.count(), 0, "filing sends nothing");

    // The approving human still needs the dialog for this invocation.
    assert_err(w.authorise(&alice, &p, Some(&id), None), Code::Policy, "no_approval");

    let consent = w.approval(&alice, w.digest, "upload");
    let decision = w.authorise(&alice, &p, Some(&id), Some(consent)).unwrap();
    let sent = w.send(&alice, decision, https(&r), CancelToken::never()).unwrap();
    assert_eq!(r.count(), 1);
    assert_eq!(r.received()[0].1, w.bytes);
    let approval = sent.request_approval.clone().unwrap();
    assert_eq!(approval.request(), id);
    assert_eq!(approval.requested_by().kind, PrincipalKind::Agent);
    assert_eq!(st(event_body(&sent), "request"), id.to_string());

    // The commit carries the egress event and RequestApproved; folded, R is approved.
    let origin = EntryOrigin::new(
        approval.approved_by().clone(),
        CommandId::parse("egress.send@1").unwrap(),
        approval.params_commitment(),
    );
    let mut committed = sent.events.clone();
    committed.push(approval.event().unwrap());
    w.book.apply_entry(&origin, &committed).unwrap();
    assert_eq!(w.book.get(&id).unwrap().state(), RequestState::Approved);

    // A second approval of the same request is refused; nothing more is sent.
    let consent = w.approval(&alice, w.digest, "upload");
    assert_err(w.authorise(&alice, &p, Some(&id), Some(consent)), Code::Policy, "bad_approval");
    assert_eq!(r.count(), 1);
}

/// REQ-002: a `script` principal (an unupgraded CLI session) files a request too.
#[test]
fn ac2_a_script_files_a_request() {
    let mut w = World::new();
    let script = w.channel(ChannelKind::Cli, "ci-bot");
    let p = params(&w.digest, "upload");
    assert!(matches!(w.authorise(&script, &p, None, None).unwrap(), EgressDecision::File(_)));
    // A Python host session is a script too.
    let python = w.channel(ChannelKind::Python, "nb");
    assert!(matches!(w.authorise(&python, &p, None, None).unwrap(), EgressDecision::File(_)));
}

// ------------------------------------------------------------------ AC-3 and refusals

/// AC-3: an unknown destination is `bad_destination` and nothing is filed or sent, for a
/// human and for an agent; a lookalike or differently cased ID does not match.
#[test]
fn ac3_unknown_destinations_are_refused_before_anything_else() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let agent = w.channel(ChannelKind::Mcp, "claude-desktop");
    for dest in
        ["nowhere", "Upload", "uploa\u{0501}", "upload ", "upload\u{200b}", "", "https://upload.example.org/inbox"]
    {
        let mut o = Obj::new();
        o.insert("artifact".into(), CValue::Str(w.digest.as_str().into()));
        o.insert("destination".into(), CValue::Str(dest.into()));
        let p = CValue::Obj(o);
        let consent = w.approval(&alice, w.digest, "upload");
        assert_err(w.authorise(&alice, &p, None, Some(consent)), Code::Policy, "bad_destination");
        assert_err(w.authorise(&agent, &p, None, None), Code::Policy, "bad_destination");
    }
    assert!(w.journal.drafts.is_empty());
}

/// AC-3: a tampered or missing artifact is `bad_artifact` and the transport is never called.
#[test]
fn ac3_a_tampered_or_missing_artifact_is_refused_and_nothing_is_sent() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "upload");
    let r = Recorder::ok();

    let mut tampered = w.bytes.clone();
    tampered[100] ^= 0x01;
    std::fs::write(w.store.path_of(&w.digest), &tampered).unwrap();
    let consent = w.approval(&alice, w.digest, "upload");
    let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
    assert_err(w.send(&alice, decision, https(&r), CancelToken::never()), Code::Validation, "bad_artifact");

    std::fs::write(w.store.path_of(&w.digest), &w.bytes[..10]).unwrap();
    let consent = w.approval(&alice, w.digest, "upload");
    let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
    assert_err(w.send(&alice, decision, https(&r), CancelToken::never()), Code::Validation, "bad_artifact");

    let missing = Digest::from_bytes([0x42; 32]);
    let p2 = params(&missing, "upload");
    let consent = w.approval(&alice, missing, "upload");
    let decision = w.authorise(&alice, &p2, None, Some(consent)).unwrap();
    assert_err(w.send(&alice, decision, https(&r), CancelToken::never()), Code::Validation, "bad_artifact");
    assert_eq!(r.count(), 0);
}

/// REQ-002: every way of lacking a matching human approval is `no_approval`.
#[test]
fn a_send_without_a_matching_human_approval_is_refused() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let bob = w.channel(ChannelKind::Gui, "bob");
    let p = params(&w.digest, "upload");

    assert_err(w.authorise(&alice, &p, None, None), Code::Policy, "no_approval");
    let other = Digest::from_bytes([0x42; 32]);
    let consent = w.approval(&alice, other, "upload");
    assert_err(w.authorise(&alice, &p, None, Some(consent)), Code::Policy, "no_approval");
    let consent = w.approval(&alice, w.digest, "lab-share");
    assert_err(w.authorise(&alice, &p, None, Some(consent)), Code::Policy, "no_approval");
    let consent = w.approval(&bob, w.digest, "upload");
    assert_err(w.authorise(&alice, &p, None, Some(consent)), Code::Policy, "no_approval");

    // The administrator changes the destination after the approval: the approval is void.
    let consent = w.approval(&alice, w.digest, "upload");
    w.destinations =
        Destinations::from_value(&destinations_value(&w.share.0, "https://elsewhere.example/inbox")).unwrap();
    assert_err(w.authorise(&alice, &p, None, Some(consent)), Code::Policy, "no_approval");
}

/// The dialog: declined, timed out, confirmed by another OS user, or asked of a non-human
/// or over MCP, gives no approval.
#[test]
fn the_dialog_gives_an_approval_only_to_the_confirming_human() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let dest = w.dest("upload").clone();
    for answer in [
        PresenceOutcome::Declined,
        PresenceOutcome::TimedOut,
        PresenceOutcome::Confirmed { os_user: OsUser::new("mallory").unwrap(), method: ConfirmMethod::GuiPrompt },
    ] {
        let d = Dialog::answering(answer);
        assert_err(EgressApproval::ask(&w.table, &alice, &d, &mut Clock, w.digest, &dest), Code::Policy, "no_approval");
    }
    for (kind, who) in [
        (ChannelKind::Mcp, "agent"),
        (ChannelKind::ServerMcp, "agent2"),
        (ChannelKind::Cli, "script"),
        (ChannelKind::Service, "svc"),
        (ChannelKind::Device, "cmm"),
        (ChannelKind::Internal, "core"),
    ] {
        let ch = w.channel(kind, who);
        let d = Dialog::confirm_as(who);
        assert_err(EgressApproval::ask(&w.table, &ch, &d, &mut Clock, w.digest, &dest), Code::Policy, "no_approval");
        assert!(d.shown.borrow().is_empty(), "{kind:?}: the dialog is not even shown");
    }
}

/// Service, device and system principals cannot send, with or without an approval.
#[test]
fn service_device_and_system_principals_are_refused() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "upload");
    for (kind, who) in [(ChannelKind::Service, "svc"), (ChannelKind::Device, "cmm"), (ChannelKind::Internal, "core")] {
        let ch = w.channel(kind, who);
        assert_err(w.authorise(&ch, &p, None, None), Code::Policy, "no_approval");
        let consent = w.approval(&alice, w.digest, "upload");
        assert_err(w.authorise(&ch, &p, None, Some(consent)), Code::Policy, "no_approval");
    }
}

/// The principal comes from the channel only: parameters naming a principal, an approver or
/// a URL are `validation/schema`; so are a malformed digest and a bad note.
#[test]
fn parameters_cannot_widen_egress() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let agent = w.channel(ChannelKind::Mcp, "claude-desktop");
    let d = w.digest;
    let long = "x".repeat(MAX_NOTE_CHARS + 1);
    for extra in [
        r#","principal":{"kind":"human","id":"alice"}"#.to_string(),
        r#","approved_by":"alice""#.to_string(),
        r#","url":"https://evil.example/x""#.to_string(),
        format!(r#","note":"{long}""#),
        r#","note":"line\nbreak""#.to_string(),
        r#","note":"bidi\u202eoverride""#.to_string(),
        r#","note":7"#.to_string(),
    ] {
        let p = json(&format!(r#"{{"artifact":"{d}","destination":"upload"{extra}}}"#));
        let consent = w.approval(&alice, d, "upload");
        assert_err(w.authorise(&alice, &p, None, Some(consent)), Code::Validation, "schema");
        assert_err(w.authorise(&agent, &p, None, None), Code::Validation, "schema");
    }
    let p = json(r#"{"artifact":"sha256:XYZ","destination":"upload"}"#);
    assert_err(w.authorise(&alice, &p, None, None), Code::Validation, "schema");
    let p = json(r#"{"destination":"upload"}"#);
    assert_err(w.authorise(&alice, &p, None, None), Code::Validation, "schema");
    let ok_note = "n".repeat(MAX_NOTE_CHARS);
    let p = json(&format!(r#"{{"artifact":"{d}","destination":"upload","note":"{ok_note}"}}"#));
    let consent = w.approval(&alice, d, "upload");
    assert!(w.authorise(&alice, &p, None, Some(consent)).is_ok());
}

/// Only `egress.send@1` passes through this authorise stage.
#[test]
fn another_definition_is_refused() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let mut v = def().to_value();
    if let CValue::Obj(o) = &mut v {
        o.insert("id".into(), CValue::Str("egress.other".into()));
    }
    let other = CommandDef::from_value(v).unwrap();
    let p = params(&w.digest, "upload");
    let consent = w.approval(&alice, w.digest, "upload");
    let view = w.book.view(HEAD);
    let got = authorise(
        &w.table,
        &alice,
        &other,
        &ValidParams::for_tests(p),
        None,
        Some(consent),
        &w.destinations,
        &view,
        &w.now,
    );
    assert_err(got, Code::Policy, "no_approval");
}

/// An `https` destination is refused while this build has no `https` transport, and nothing
/// is read or sent.
#[test]
fn a_kind_without_a_transport_is_a_bad_destination() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "upload");
    let consent = w.approval(&alice, w.digest, "upload");
    let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
    let t = NetworkRootTransport::new();
    let only_root = Transports { network_root: Some(&t), https: None };
    assert_err(w.send(&alice, decision, only_root, CancelToken::never()), Code::Policy, "bad_destination");
    assert!(std::fs::read_dir(&w.share.0).unwrap().next().is_none());
}

/// Sending checks the permit class and the executing principal.
#[test]
fn a_send_needs_an_x1_permit_and_the_approving_human() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let bob = w.channel(ChannelKind::Gui, "bob");
    let p = params(&w.digest, "upload");
    let r = Recorder::ok();

    let consent = w.approval(&alice, w.digest, "upload");
    let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
    assert_err(w.send(&bob, decision, https(&r), CancelToken::never()), Code::Policy, "no_approval");

    let consent = w.approval(&alice, w.digest, "upload");
    let EgressDecision::Send(cleared) = w.authorise(&alice, &p, None, Some(consent)).unwrap() else { panic!() };
    let mut v = def().to_value();
    if let CValue::Obj(o) = &mut v {
        o.insert("side_effect".into(), CValue::Str("I1".into()));
        o.insert("mcp_projection".into(), CValue::Str("exposed".into()));
        o.insert("network".into(), CValue::Str("device".into()));
    }
    let i1 = CommandDef::from_value(v).unwrap();
    let mut scope = PermitScope::new();
    let permit = issue_permit_for_tests(&i1, &mut scope).unwrap();
    let ctx = HandlerCtx::derive(&w.table, &alice, 0, CancelToken::never()).unwrap();
    assert_err(
        EgressSender::new(&w.store, https(&r)).send(*cleared, &w.destinations, &permit, &ctx),
        Code::Policy,
        "class_forbidden",
    );
    assert_eq!(r.count(), 0);
}

// ------------------------------------------------------------------ REQ-006

/// REQ-006: a failed send is recorded as `failed` and not retried; remote text is bounded.
#[test]
fn a_failed_send_is_recorded_and_never_retried() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "upload");
    let noisy = format!("refused\u{202e}{}", "z".repeat(2 * MAX_RECEIPT_CHARS));
    let r = Recorder::new(SendOutcome::Failed { reason: Untrusted::new(noisy) });
    let consent = w.approval(&alice, w.digest, "upload");
    let sent = w.send(&alice, w.authorise(&alice, &p, None, Some(consent)).unwrap(), https(&r), CancelToken::never());
    let sent = sent.unwrap();
    assert_eq!(r.count(), 1, "exactly one attempt");
    assert_eq!(sent.outcome, Outcome::Failed);
    let failure = st(event_body(&sent), "failure");
    assert_eq!(failure.chars().count(), MAX_RECEIPT_CHARS);
    assert!(!failure.contains('\u{202e}'));
    check_schemas(&p, &sent);

    let long = Recorder::new(SendOutcome::Sent { receipt: Some(Untrusted::new("r".repeat(5000))) });
    let consent = w.approval(&alice, w.digest, "upload");
    let sent =
        w.send(&alice, w.authorise(&alice, &p, None, Some(consent)).unwrap(), https(&long), CancelToken::never());
    let sent = sent.unwrap();
    assert_eq!(st(obj(&sent.result), "remote_receipt").chars().count(), MAX_RECEIPT_CHARS);
    check_schemas(&p, &sent);
}

/// REQ-006: a cancellation before the send is recorded as `cancelled` and nothing is sent; a
/// transport reporting cancellation is recorded the same way.
#[test]
fn a_cancelled_send_is_recorded() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "upload");
    let r = Recorder::ok();
    let source = CancelSource::new();
    source.cancel();
    let consent = w.approval(&alice, w.digest, "upload");
    let sent = w.send(&alice, w.authorise(&alice, &p, None, Some(consent)).unwrap(), https(&r), source.token());
    let sent = sent.unwrap();
    assert_eq!((sent.outcome, r.count()), (Outcome::Cancelled, 0));
    assert_eq!(st(event_body(&sent), "outcome"), "cancelled");
    check_schemas(&p, &sent);

    let c = Recorder::new(SendOutcome::Cancelled);
    let consent = w.approval(&alice, w.digest, "upload");
    let sent = w.send(&alice, w.authorise(&alice, &p, None, Some(consent)).unwrap(), https(&c), CancelToken::never());
    assert_eq!(sent.unwrap().outcome, Outcome::Cancelled);
    assert_eq!(c.count(), 1);
}

// ------------------------------------------------------------------ review of #336

/// Maintainer decision 1: a digest naming a chunked object (its manifest) is `bad_artifact`;
/// the manifest bytes are never sent.
#[test]
fn a_chunked_object_is_refused_and_its_manifest_is_never_sent() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let big: Vec<u8> = (0..(8 * 1024 * 1024 + 4096)).map(|i| (i % 251) as u8).collect();
    let addr = w.store.put_bytes(&big, PutOpts::default()).unwrap();
    assert!(matches!(addr, fid_store::cas::Address::Object(_)), "stored as a chunked object");
    let digest = addr.digest();
    let p = params(&digest, "upload");
    let r = Recorder::ok();
    let consent = w.approval(&alice, digest, "upload");
    let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
    assert_err(w.send(&alice, decision, https(&r), CancelToken::never()), Code::Validation, "bad_artifact");
    assert_eq!(r.count(), 0);
}

/// EGRESS REQ-003 with CORE.STO.CAS REQ-010 (maintainer decisions on #336 and #357): a
/// digest that names a manifest is `bad_artifact` and nothing is sent, over HTTPS or to a
/// network root. A `put`-shaped manifest is expanded by `read_object`, and the expanded bytes
/// do not hash to the approved digest; a manifest of another shape is plain data that does
/// hash to it, and is refused because it is a manifest.
#[test]
fn a_manifest_digest_is_never_sent() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let big: Vec<u8> = (0..(8 * 1024 * 1024 + 4096)).map(|i| (i % 251) as u8).collect();
    let object = w.store.put_bytes(&big, PutOpts::default()).unwrap().digest();
    let chunk = w.store.put_bytes(b"0123456789", PutOpts::default()).unwrap().digest();
    let manifest = format!(
        "{{\"chunk_size\":\"10\",\"chunks\":[\"{}\"],\"format\":\"fid-chunks@1\",\"size\":\"10\"}}",
        chunk.as_str()
    )
    .into_bytes();
    assert!(fid_store::ChunkManifest::parse(&manifest).is_ok());
    let planted = Digest::from_bytes(fid_canon::digest_bytes(&manifest).0);
    let path = w.store.path_of(&planted);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &manifest).unwrap();
    for digest in [object, planted] {
        for dest in ["upload", "lab-share"] {
            let p = params(&digest, dest);
            let r = Recorder::ok();
            let t = NetworkRootTransport::new();
            let transports =
                if dest == "upload" { https(&r) } else { Transports { network_root: Some(&t), https: None } };
            let consent = w.approval(&alice, digest, dest);
            let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
            assert_err(w.send(&alice, decision, transports, CancelToken::never()), Code::Validation, "bad_artifact");
            assert_eq!(r.count(), 0, "{digest} {dest}");
            assert_eq!(std::fs::read_dir(&w.share.0).unwrap().count(), 0, "nothing written to the share");
        }
    }
}

fn partials(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".partial-"))
        .collect()
}

fn root_send(w: &mut World, t: &NetworkRootTransport) -> Sent {
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "lab-share");
    let consent = w.approval(&alice, w.digest, "lab-share");
    let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
    let sent =
        w.send(&alice, decision, Transports { network_root: Some(t), https: None }, CancelToken::never()).unwrap();
    check_schemas(&p, &sent);
    sent
}

/// Maintainer decision 2: a write that fails in the middle leaves neither the final file nor
/// the partial file, and is recorded as `failed`.
#[test]
fn a_failed_network_root_write_leaves_nothing_behind() {
    let mut w = World::new();
    let t = NetworkRootTransport::failing_after_for_tests(1000);
    let sent = root_send(&mut w, &t);
    assert_eq!(sent.outcome, Outcome::Failed);
    assert!(!w.share.0.join(w.digest.hex()).exists());
    assert!(partials(&w.share.0).is_empty());
    assert!(std::fs::read_dir(&w.share.0).unwrap().next().is_none());
}

/// Maintainer decision 2: an existing file of the digest's name is hashed, never replaced:
/// the same content is `sent`, other content is `failed` and stays as it was.
#[test]
fn an_existing_network_root_file_is_hashed_and_never_replaced() {
    let mut w = World::new();
    let target = w.share.0.join(w.digest.hex());
    std::fs::write(&target, &w.bytes).unwrap();
    let t = NetworkRootTransport::new();
    let same = root_send(&mut w, &t);
    assert_eq!(same.outcome, Outcome::Sent);
    assert!(boolean(event_body(&same), "already_present"), "nothing was written");
    assert!(partials(&w.share.0).is_empty());

    std::fs::write(&target, b"someone else's bytes").unwrap();
    let sent = root_send(&mut w, &t);
    assert_eq!(sent.outcome, Outcome::Failed);
    assert_eq!(std::fs::read(&target).unwrap(), b"someone else's bytes");
    assert!(!boolean(event_body(&sent), "already_present"));
    assert!(partials(&w.share.0).is_empty());
}

/// Runs the `network_root` transport for `digest` on another thread and gives up after 30
/// seconds, so a send that blocks (on a FIFO) fails the test instead of hanging it.
fn transport_send_with_deadline(share: &Path, t: NetworkRootTransport, digest: Digest, bytes: Vec<u8>) -> SendOutcome {
    let share = share.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let destinations = Destinations::from_value(&destinations_value(&share, UPLOAD_URL)).unwrap();
        let d = def();
        let mut scope = PermitScope::new();
        let permit = issue_permit_for_tests(&d, &mut scope).unwrap();
        let dest = destinations.lookup("lab-share").unwrap();
        let _ = tx.send(t.send(&permit, dest, &digest, &bytes, &CancelToken::never()));
    });
    rx.recv_timeout(Duration::from_secs(30)).expect("the network_root send blocked")
}

fn file_symlink(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }
}

/// A folder link: a symbolic link on Unix, a junction on Windows (no privilege needed).
fn dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// #336 review: a symbolic link planted at `<hex>` is never followed, even when its target
/// holds exactly the artifact: the send is `failed`, not `sent`, and the link stays.
#[test]
fn a_planted_symlink_at_the_target_name_is_never_followed() {
    let mut w = World::new();
    let elsewhere = Temp::new("elsewhere");
    let outside = elsewhere.0.join("same-bytes");
    std::fs::write(&outside, &w.bytes).unwrap();
    let target = w.share.0.join(w.digest.hex());
    if !file_symlink(&target, &outside) {
        // Windows without the symbolic-link privilege; the junction test still runs.
        return;
    }
    let t = NetworkRootTransport::new();
    let got = transport_send_with_deadline(&w.share.0, t, w.digest, w.bytes.clone());
    assert!(matches!(got, SendOutcome::Failed { .. }), "{got:?}");
    let sent = root_send(&mut w, &t);
    assert_eq!(sent.outcome, Outcome::Failed);
    assert!(!boolean(event_body(&sent), "already_present"));
    assert!(std::fs::symlink_metadata(&target).unwrap().file_type().is_symlink(), "the link is left as it is");
    assert_eq!(std::fs::read(&outside).unwrap(), w.bytes);
    assert!(partials(&w.share.0).is_empty());
}

/// #336 review: a folder link or junction planted at `<hex>` is `failed`, not followed.
#[test]
fn a_planted_folder_link_or_junction_at_the_target_name_is_failed() {
    let w = World::new();
    let elsewhere = Temp::new("elsewhere-dir");
    let target = w.share.0.join(w.digest.hex());
    assert!(dir_link(&target, &elsewhere.0), "the folder link could be made");
    let got = transport_send_with_deadline(&w.share.0, NetworkRootTransport::new(), w.digest, w.bytes.clone());
    assert!(matches!(got, SendOutcome::Failed { .. }), "{got:?}");
    assert!(std::fs::read_dir(&elsewhere.0).unwrap().next().is_none(), "nothing was written through the link");
    assert!(partials(&w.share.0).is_empty());
}

/// #336 review, Unix: a FIFO planted at `<hex>` neither blocks the send nor counts as
/// `sent`. (Windows has no FIFO in the file system; skipped there.)
#[cfg(unix)]
#[test]
fn a_planted_fifo_at_the_target_name_does_not_block() {
    use std::os::unix::fs::FileTypeExt;
    let mut w = World::new();
    let target = w.share.0.join(w.digest.hex());
    let made = std::process::Command::new("mkfifo").arg(&target).status().map(|s| s.success()).unwrap_or(false);
    assert!(made, "mkfifo is available on the Unix runners");
    let got = transport_send_with_deadline(&w.share.0, NetworkRootTransport::new(), w.digest, w.bytes.clone());
    assert!(matches!(got, SendOutcome::Failed { .. }), "{got:?}");
    let sent = root_send(&mut w, &NetworkRootTransport::new());
    assert_eq!(sent.outcome, Outcome::Failed);
    assert!(!boolean(event_body(&sent), "already_present"));
    assert!(std::fs::symlink_metadata(&target).unwrap().file_type().is_fifo(), "the FIFO is left as it is");
    assert!(partials(&w.share.0).is_empty());
}

/// #336 review: `<hex>` is hashed again after the link; a file changed in between is
/// `failed`, not `sent`, and left as it is.
#[test]
fn a_file_changed_after_the_link_is_failed() {
    let mut w = World::new();
    let sent = root_send(&mut w, &NetworkRootTransport::tampering_after_link_for_tests());
    assert_eq!(sent.outcome, Outcome::Failed);
    assert!(st(event_body(&sent), "failure").contains("does not verify"), "{:?}", event_body(&sent));
    assert!(!boolean(event_body(&sent), "already_present"));
    assert!(w.share.0.join(w.digest.hex()).exists(), "the changed file is not removed");
    assert!(partials(&w.share.0).is_empty());
}

/// Maintainer decision 4: an approval older than five minutes is refused at authorise; a
/// stale approval is no approval (`policy/no_approval`, #336 review).
#[test]
fn a_stale_approval_is_refused() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "upload");
    assert_eq!(APPROVAL_MAX_AGE.as_secs(), 300);
    w.now = later(300);
    let consent = w.approval(&alice, w.digest, "upload");
    assert!(w.authorise(&alice, &p, None, Some(consent)).is_ok(), "exactly five minutes is accepted");
    // Too old, and a clock reading before the approval.
    for now in [later(301), EntryTime { mono_ns: 999, ..time() }] {
        w.now = now;
        let consent = w.approval(&alice, w.digest, "upload");
        assert_err(w.authorise(&alice, &p, None, Some(consent)), Code::Policy, "no_approval");
    }
}

/// Maintainer decision 7: the destination is checked again at send time against the setting
/// as read then; a changed configuration is `no_approval`, a removed one `bad_destination`.
#[test]
fn the_destination_is_rechecked_at_send_time() {
    let mut w = World::new();
    let alice = w.channel(ChannelKind::Gui, "alice");
    let p = params(&w.digest, "upload");
    let r = Recorder::ok();

    let consent = w.approval(&alice, w.digest, "upload");
    let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
    w.destinations =
        Destinations::from_value(&destinations_value(&w.share.0, "https://elsewhere.example/inbox")).unwrap();
    assert_err(w.send(&alice, decision, https(&r), CancelToken::never()), Code::Policy, "no_approval");

    w.destinations = Destinations::from_value(&destinations_value(&w.share.0, UPLOAD_URL)).unwrap();
    let consent = w.approval(&alice, w.digest, "upload");
    let decision = w.authorise(&alice, &p, None, Some(consent)).unwrap();
    w.destinations = Destinations::default();
    assert_err(w.send(&alice, decision, https(&r), CancelToken::never()), Code::Policy, "bad_destination");
    assert_eq!(r.count(), 0);
}

/// Maintainer decision 5: the settings path reads the machine file; a value at user scope is
/// ignored. `from_machine_settings` resolves the real machine file and fails closed.
#[test]
fn destinations_come_from_the_machine_settings_file() {
    let dir = Temp::new("settings");
    let share = Temp::new("share");
    let file = |values: CValue| {
        let mut v = Obj::new();
        v.insert(DESTINATIONS_KEY.into(), values);
        let mut o = Obj::new();
        o.insert("format".into(), CValue::Str("fid-settings/1".into()));
        o.insert("values".into(), CValue::Obj(v));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap()
    };
    std::fs::write(dir.0.join("machine.json"), file(destinations_value(&share.0, UPLOAD_URL))).unwrap();
    std::fs::write(dir.0.join("user.json"), file(destinations_value(&share.0, "https://user.example/x"))).unwrap();
    let machine = FileStore::new(Scope::Machine, dir.0.join("machine.json")).unwrap();
    let user = FileStore::new(Scope::User, dir.0.join("user.json")).unwrap();
    let (got, warnings) =
        Destinations::from_settings(&Sources { project: None, user: &user, machine: &machine }).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got.lookup("upload").unwrap().target(), UPLOAD_URL);
    assert!(!warnings.is_empty(), "the user-scope value is reported as not allowed");

    // On a CI runner there is no machine settings file: the result is empty or an error,
    // never a destination from anywhere else.
    if let Ok((d, _)) = Destinations::from_machine_settings() {
        assert!(d.lookup("upload").map_or(true, |x| x.target() != UPLOAD_URL), "nothing from the test files");
    }
}

/// Maintainer decision 8: `network_root` paths with `.`/`..`, controls or device and verbatim
/// prefixes are refused.
#[test]
fn network_root_paths_are_checked() {
    let c = cases();
    let mut checked = 0;
    for case in arr(obj(&c), "root_paths") {
        let o = obj(case);
        if !applies(st(o, "platform")) {
            continue;
        }
        let got = check_root_path(st(o, "path"));
        assert_eq!(got.is_ok(), boolean(o, "valid"), "{}: {:?}", st(o, "name"), got);
        checked += 1;
    }
    assert!(checked >= 20);
}
