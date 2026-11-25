//! AUTO.SURF.INPROC against a real `fid-bus` `LocalPort` (D-035, CORE.BUS.PRIN). Not on wasm32:
//! the bus links SQLite.
//!
//! Two read-only buses over the same session host, which has no presence provider:
//! - `with_hidden` holds `demo.thing@1` (lifecycle `hidden`) and `demo.thing@2`;
//! - `without` holds only `demo.thing@2`.
//!
//! A call of `demo.thing@1` gets the bus's real `hidden_command` refusal from the first bus and
//! its real `not_found/unknown_major` refusal from the second. The transport must report both
//! with the same envelope bytes. Expected values are hand-labelled from the leaf and from
//! CORE.BUS.PORT REQ-008: no presence provider means `upgrade_refused/declined`, and the port
//! stays `script`.
#![cfg(not(target_family = "wasm"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::InternalReason;
use fid_bus::handler::{EntityId, EntityRef, Event, Handler, HandlerCtx, HandlerError, HandlerSet};
use fid_bus::handler::{SettingKey, StateView, ValidParams};
use fid_bus::pipeline::{AppendFailure, AppendOutcome, Bus, BusEnv, BusParts, RecordJournal};
use fid_bus::policy::projection::RequestId;
use fid_bus::port::{CallParts, LocalBus, LocalPort, SessionHost, NO_PRESENCE_PROVIDER};
use fid_bus::principal::{Channel, OsUser, SessionId, UpgradeClock};
use fid_bus::route::SessionRouter;
use fid_bus::validate::{CommandTable, NoRoots};
use fid_bus::{BusError, Detail};
use fid_bus_api::{ApiReply, ApiStage, BusCall, BusPort, PortKind, PrincipalKind};
use fid_canon::{parse, CValue, Digest};
use fid_journal::branch::BranchName;
use fid_journal::stream::{format_utc, ActivityLog, ActivityLogConfig, Streams};
use fid_journal::{AppendRequest, EntryHash, EntryTime, Manifests, GENESIS_PREV};
use fid_registry::{Catalogue, CommandDef, ContextTypes, PredContext};
use fid_surface::inproc::{envelope_for, hidden_as_unknown_major, InProcTransport};
use fid_surface::jobs::{CountingKeySource, ManualClock, Reason, Tracker};
use fid_surface::Envelope;
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MANIFEST: Digest = Digest([7; 32]);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifests() -> Manifests {
    Manifests { registry: MANIFEST, kernel: Digest([2; 32]) }
}

fn def(major: u32, state: &str) -> CommandDef {
    let json = format!(
        r#"{{"id":"demo.thing","major":{major},"title":"Show a thing","description":"D",
"params_schema":{{"type":"object","additionalProperties":false,"properties":{{}}}},
"result_schema":{{"type":"object"}},"side_effect":"R0","mcp_projection":"exposed","determinism":"D0",
"idempotency":"none","fs_effect":"none","undo":"not_undoable","preconditions":[],"effects":[],
"algorithms":[],"exposure":["cli","gui","mcp"],"human_only":false,"network":"none","cli_bare":false,
"lifecycle":{{"state":"{state}","since":"2026-11-21"}},"web":"available"}}"#
    );
    CommandDef::from_value(parse(json.as_bytes()).unwrap()).unwrap()
}

/// A read: no events, no result.
struct Read;

impl Handler for Read {
    fn run(&self, _view: &dyn StateView, _params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        Ok(Vec::new())
    }
}

/// A journal nothing is committed to (every command here is R0).
struct NoJournal;

impl RecordJournal for NoJournal {
    fn tail(&self) -> Result<Option<(u64, Digest)>, BusError> {
        Ok(None)
    }
    fn branch_head(&self, _branch: &str) -> Result<Option<Digest>, BusError> {
        Ok(None)
    }
    fn append(&self, _req: AppendRequest) -> Result<EntryHash, AppendFailure> {
        Err(AppendFailure {
            error: BusError::Internal(InternalReason::JournalUnavailable, Detail::new("read-only test journal")),
            outcome: AppendOutcome::NotAppended,
        })
    }
}

struct View;

impl StateView for View {
    fn entity(&self, _id: &EntityId) -> Option<EntityRef<'_>> {
        None
    }
    fn head(&self) -> Digest {
        GENESIS_PREV
    }
    fn setting(&self, _key: &SettingKey) -> Option<CValue> {
        None
    }
}

struct Parts;

impl CallParts for Parts {
    fn inputs(&self, _channel: &dyn Channel) -> Result<(Box<dyn StateView>, Box<dyn PredContext>), BusError> {
        Ok((Box::new(View), Box::new(NoRoots)))
    }
}

struct Env {
    mono: AtomicU64,
}

impl BusEnv for Env {
    fn now(&self) -> EntryTime {
        let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let mono_ns = self.mono.fetch_add(1, Ordering::SeqCst) + 1;
        EntryTime { utc: format_utc(since), mono_ns, sync: "ntp".into() }
    }
    fn seed(&self) -> u64 {
        42
    }
    fn request_id(&self) -> Result<RequestId, BusError> {
        let n = u128::from(self.mono.fetch_add(1, Ordering::SeqCst));
        RequestId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000u128 | (n & 0xffff_ffff)))
    }
}

struct Clock(u64);

impl UpgradeClock for Clock {
    fn now(&mut self) -> EntryTime {
        self.0 += 1_000_000;
        EntryTime { utc: "2026-11-25T10:00:00Z".into(), mono_ns: self.0, sync: "ntp".into() }
    }
}

/// A read-only bus with no upgrade journal over the given majors of `demo.thing`.
fn bus(majors: &[(u32, &str)]) -> Arc<LocalBus> {
    let defs: Vec<CommandDef> = majors.iter().map(|(m, s)| def(*m, s)).collect();
    let catalogue =
        Catalogue::from_json(&std::fs::read(root().join("fixtures/core/bus/unit/catalogue.json")).unwrap()).unwrap();
    let table = CommandTable::build(MANIFEST, defs.clone(), catalogue, &ContextTypes::new()).unwrap();
    assert!(table.compile_errors().is_empty(), "{:?}", table.compile_errors());
    let mut handlers = HandlerSet::new();
    for d in defs {
        handlers.register(d, Box::new(Read)).unwrap();
    }
    // Recording is off, so the Activity Log writes nothing and its folder is never created.
    let alog = std::env::temp_dir().join(format!("fid-surface-inproc-alog-{}", std::process::id()));
    let activity = Arc::new(ActivityLog::new(alog, ActivityLogConfig::default(), manifests()));
    let bus = Bus::new(BusParts {
        table: Arc::new(table),
        handlers,
        router: Arc::new(SessionRouter::new(BranchName::parse("main").unwrap())),
        journal: Arc::new(NoJournal),
        streams: Streams::new(activity),
        env: Arc::new(Env { mono: AtomicU64::new(1) }),
        stream: "record".to_string(),
        manifests: manifests(),
    });
    Arc::new(LocalBus::new(bus, Arc::new(Parts), None))
}

/// A host with no presence provider (nobody can be asked).
fn host() -> Arc<SessionHost> {
    Arc::new(SessionHost::new(OsUser::new("user-a").unwrap(), None, Box::new(Clock(5_000_000_000))).unwrap())
}

fn port(host: &Arc<SessionHost>, label: &str, bus: &Arc<LocalBus>) -> LocalPort {
    host.open(PortKind::Cli, SessionId::new(label).unwrap(), Arc::clone(bus)).unwrap()
}

fn with_hidden() -> Arc<LocalBus> {
    bus(&[(1, "hidden"), (2, "active")])
}

fn without() -> Arc<LocalBus> {
    bus(&[(2, "active")])
}

fn thing1() -> CommandId {
    CommandId::parse("demo.thing@1").unwrap()
}

fn call(command: &CommandId) -> BusCall {
    BusCall { command: command.clone(), params: parse(b"{}").unwrap(), dry_run: false }
}

fn encoded(e: &Envelope) -> String {
    String::from_utf8(e.encode().unwrap()).unwrap()
}

/// D-035: the bus's real `hidden_command` refusal and its real `not_found/unknown_major`
/// refusal of the same ID give the same envelope bytes, which are those of
/// `hidden_as_unknown_major`.
#[test]
fn hidden_major_bytes_equal_the_real_unknown_major_refusal() {
    let host = host();
    let cmd = thing1();
    let hidden = port(&host, "cli-1", &with_hidden()).call(&call(&cmd));
    let unknown = port(&host, "cli-2", &without()).call(&call(&cmd));
    let (ApiReply::Failed(h), ApiReply::Failed(u)) = (&hidden, &unknown) else {
        panic!("not two refusals: {hidden:?} {unknown:?}")
    };
    assert_eq!((h.code(), h.reason(), h.stage()), ("hidden_command", "hidden_major", ApiStage::Resolve));
    assert_eq!((u.code(), u.reason(), u.stage()), ("not_found", "unknown_major", ApiStage::Resolve));
    assert_ne!(h.detail(), u.detail(), "the bus's own details differ; the transport must hide that");

    let hidden_bytes = encoded(&envelope_for(&cmd, &hidden));
    let unknown_bytes = encoded(&envelope_for(&cmd, &unknown));
    assert_eq!(hidden_bytes, unknown_bytes);
    assert_eq!(hidden_bytes, encoded(&hidden_as_unknown_major(&cmd)));
    assert_eq!(
        unknown_bytes,
        concat!(
            r#"{"command_id":"demo.thing@1","envelope_version":1,"error":{"code":"not_found","details":"#,
            r#"{"bus_code":"not_found","bus_origin":"pipeline","bus_reason":"unknown_major"},"#,
            r#""message":"\"demo.thing\"@1: unknown major"},"outcome":"failed"}"#
        )
    );

    // The visible major still runs on both buses.
    let cmd2 = CommandId::parse("demo.thing@2").unwrap();
    for bus in [with_hidden(), without()] {
        let reply = port(&host, "cli-3", &bus).call(&call(&cmd2));
        assert!(matches!(reply, ApiReply::Computed { .. }), "{reply:?}");
    }
}

/// Over a real port with no presence provider, through the transport and the tracker:
/// `confirm_human` is `upgrade_refused/declined`, the port stays `script`, and a hidden major
/// finishes with the same bytes as a missing one.
#[test]
fn real_port_without_presence_fails_closed_and_hides_hidden_majors() {
    let host = host();
    let track = |label: &str, bus: &Arc<LocalBus>| {
        let transport =
            InProcTransport::new(Box::new(port(&host, label, bus)), Box::new(CountingKeySource::new(0x0190_8765_4321)));
        Tracker::new(transport, ManualClock::new(), CountingKeySource::new(0x0190_1234_5678))
    };

    let mut a = track("cli-1", &with_hidden());
    assert_eq!(a.transport().principal_kind(), PrincipalKind::Script);
    let err = a.transport_mut().confirm_human().unwrap_err();
    assert_eq!((err.code(), err.reason(), err.detail()), ("upgrade_refused", "declined", NO_PRESENCE_PROVIDER));
    assert_eq!(err.stage(), ApiStage::Session);
    assert_eq!(a.transport().principal_kind(), PrincipalKind::Script, "a refused upgrade leaves the port script");
    assert_eq!(a.transport().channel().kind, PortKind::Cli);

    let mut b = track("cli-2", &without());
    let finish = |t: &mut Tracker<InProcTransport, ManualClock, CountingKeySource>| {
        let id = t.submit(thing1(), parse(b"{}").unwrap(), Duration::from_millis(200)).unwrap();
        t.pump(&mut ());
        let report = t.take(id).unwrap();
        assert_eq!(report.reason, Reason::Final);
        encoded(&report.envelope)
    };
    assert_eq!(finish(&mut a), finish(&mut b));
    assert_eq!(a.transport().principal_kind(), PrincipalKind::Script);
}
