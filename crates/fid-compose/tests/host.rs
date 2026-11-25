//! AC-3, AC-7 and AC-8 of spec node AUTO.SURF.COMPOSE, the raw-path rule for a pending
//! upgrade (review of #624), and D-035 and D-045 end to end, over real projects in temporary
//! folders. Expected values are hand-labelled from the leaf, CORE.BUS.PORT and
//! AUTO.SURF.INPROC.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_bus::port::LocalPort;
use fid_bus::principal::SessionId;
use fid_bus::validate::Compiled;
use fid_bus_api::{ApiError, ApiReply, ApiStage, BusCall, BusPort, PortKind, PrincipalKind};
use fid_canon::{parse, CValue, Digest};
use fid_compose::{not_wired_detail, Composition, InProcessHost, OpenedProject, ProjectRef, HANDLER_SOURCES, UNWIRED};
use fid_registry::{mcp_projection, CommandDef, Projection};
use fid_store::{OpenMode, ProjectLocation};
use fid_surface::inproc::{envelope_for, hidden_as_unknown_major};
use fid_surface::{ErrorCode, Outcome};
use fid_types::CommandId;
use fid_types::SideEffect;
use std::sync::Arc;

fn label() -> CValue {
    obj(&[("label", CValue::Str("P1".into()))])
}

fn encoded(e: &fid_surface::Envelope) -> String {
    String::from_utf8(e.encode().unwrap()).unwrap()
}

/// An UNWIRED major a `cli` script may call with `{}`: R0 or R1, not human-only, no
/// precondition, no required parameter.
fn callable_unwired(comp: &Composition) -> String {
    UNWIRED
        .iter()
        .map(|(id, _)| *id)
        .find(|id| {
            let d = comp.def(id).unwrap();
            matches!(d.side_effect, SideEffect::R0 | SideEffect::R1)
                && !d.human_only
                && d.preconditions.is_empty()
                && conforms(&d.params_schema, &obj(&[]))
        })
        .expect("an UNWIRED R0 major callable with {}")
        .to_string()
}

/// AC-3: a read-only project opened by handle, a `cli` port: `project.list_elements@1` is
/// `computed` with a result that validates against `schema/element-list.schema.json`; an
/// UNWIRED major fails `internal_error` from the handler; `demo.no_such@1` is `not_found` from
/// the pipeline.
#[test]
fn ac3_read_only_project_through_the_transport() {
    let base = temp_dir("ac3");
    let dir = new_project(&base);
    let mut h = host(plain_host(), PortKind::Cli, roots("work", "import", &base));
    h.open_project(ProjectRef::Handle("root:work/p".into()), OpenMode::Read).unwrap();
    assert_eq!(lock_record(&dir), "", "a read-only open takes no lock");
    let unwired = callable_unwired(&Composition::workspace().unwrap());
    let mut t = tracker(h.into_transport());

    let env = send(&mut t, "project.list_elements@1", obj(&[]));
    assert_eq!(env.outcome, Outcome::Computed, "{env:?}");
    let schema =
        parse(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/element-list.schema.json")).unwrap())
            .unwrap();
    let result = env.result.clone().unwrap();
    assert!(conforms(&schema, &result), "{result:?}");
    assert!(Compiled::compile(&schema).is_ok());

    let env = send(&mut t, &unwired, obj(&[]));
    assert_eq!(env.outcome, Outcome::Failed);
    let err = env.error.clone().unwrap();
    assert_eq!(err.code, ErrorCode::InternalError);
    assert_eq!(err.message, not_wired_detail(&unwired));
    assert_eq!(detail(&env, "bus_code").as_deref(), Some("internal"));
    assert_eq!(detail(&env, "bus_reason").as_deref(), Some("invariant_violated"));
    assert_eq!(detail(&env, "bus_origin").as_deref(), Some("handler"));

    let env = send(&mut t, "demo.no_such@1", obj(&[]));
    assert_eq!(env.error.clone().unwrap().code, ErrorCode::NotFound);
    assert_eq!(detail(&env, "bus_origin").as_deref(), Some("pipeline"));
    assert!(journal_headers(&dir).is_empty(), "reads write nothing");
}

/// AC-7 (error path): a `script` port and a raw path: `raw_path_refused`, no lock taken and
/// nothing opened (an existing project keeps an empty lock record; a missing folder is not
/// created).
#[test]
fn ac7_a_script_port_gets_no_raw_path() {
    let base = temp_dir("ac7-script");
    let dir = new_project(&base);
    let mut h = host(plain_host(), PortKind::Cli, fid_bus::policy::fs::Roots::default());
    assert_eq!(h.principal_kind(), PrincipalKind::Script);
    for mode in [OpenMode::Write, OpenMode::Read] {
        let e = h.open_project(ProjectRef::RawPath(dir.clone()), mode).unwrap_err();
        assert_eq!((e.code(), e.reason()), ("policy", "raw_path_refused"), "{e}");
    }
    assert_eq!(lock_record(&dir), "");
    assert!(!h.has_project());
    drop(ProjectLocation::open(&dir, OpenMode::Write).unwrap());

    let missing = base.join("missing");
    let e = h.open_project(ProjectRef::RawPath(missing.clone()), OpenMode::Write).unwrap_err();
    assert_eq!(e.reason(), "raw_path_refused");
    assert!(!missing.exists());

    // A raw path passed as a handle is refused by the file policy for the same principal.
    let e = h.open_project(ProjectRef::Handle(dir.to_string_lossy().to_string()), OpenMode::Read).unwrap_err();
    assert_eq!(e.reason(), "raw_path_refused");
    let e = h.open_project(ProjectRef::Handle("root:nope/p".into()), OpenMode::Read).unwrap_err();
    assert_eq!(e.reason(), "bad_root");
    assert!(!h.has_project());
}

/// AC-7: a fake provider confirms on the no-project bus; the upgrade stays pending; opening a
/// raw-path project writable records one `principal_upgrade` entry before the first command
/// entry, and a W2 command then commits with principal kind `human`.
#[test]
fn ac7_a_confirmed_upgrade_is_journalled_before_the_first_command() {
    let base = temp_dir("ac7-human");
    let dir = new_project(&base);
    let mut h = host(confirming_host(), PortKind::Cli, fid_bus::policy::fs::Roots::default());
    h.confirm_human().unwrap();
    assert_eq!(h.principal_kind(), PrincipalKind::Script, "pending, not recorded: still script");
    h.open_project(ProjectRef::RawPath(dir.clone()), OpenMode::Write).unwrap();
    assert_eq!(h.principal_kind(), PrincipalKind::Human);
    let mut t = tracker(h.into_transport());
    let env = send(&mut t, "project.create_part@1", label());
    assert_eq!(env.outcome, Outcome::Committed, "{env:?}");
    drop(t);

    let headers = journal_headers(&dir);
    let kinds: Vec<String> = headers.iter().map(|h| text_at(h, &["kind"]).unwrap()).collect();
    assert_eq!(kinds, ["principal_upgrade", "command"]);
    assert_eq!(text_at(&headers[1], &["principal", "kind"]).as_deref(), Some("human"));
    assert_eq!(text_at(&headers[1], &["command", "id"]).as_deref(), Some("project.create_part"));
    assert_eq!(text_at(&headers[1], &["command", "class"]).as_deref(), Some("W2"));
}

/// Review of #624: a port whose confirmed upgrade is pending and cannot be journalled gets no
/// raw path. A read-only project has no upgrade journal, so the open is refused, the project
/// released and the port stays `script`; the upgrade stays pending and is recorded once a
/// writable open can journal it. A project locked by another writer is refused with `locked`
/// and its holder.
#[test]
fn a_pending_upgrade_that_cannot_be_journalled_gets_no_raw_path() {
    let base = temp_dir("pending");
    let dir = new_project(&base);
    let mut h = host(confirming_host(), PortKind::Cli, fid_bus::policy::fs::Roots::default());
    h.confirm_human().unwrap();

    let e = h.open_project(ProjectRef::RawPath(dir.clone()), OpenMode::Read).unwrap_err();
    assert_eq!(e.reason(), "raw_path_refused", "{e}");
    assert!(!h.has_project());
    assert_eq!(h.principal_kind(), PrincipalKind::Script);
    assert!(journal_headers(&dir).is_empty());

    let (held, lock) = ProjectLocation::open(&dir, OpenMode::Write).unwrap();
    let e = h.open_project(ProjectRef::RawPath(dir.clone()), OpenMode::Write).unwrap_err();
    assert_eq!(e.code(), "locked", "{e}");
    match &e {
        fid_compose::HostError::Store { holder: Some(holder), .. } => assert_eq!(holder.pid, std::process::id()),
        other => panic!("no holder: {other:?}"),
    }
    assert!(!h.has_project());
    assert_eq!(h.principal_kind(), PrincipalKind::Script);
    drop((held, lock));

    h.open_project(ProjectRef::RawPath(dir.clone()), OpenMode::Write).unwrap();
    assert_eq!(h.principal_kind(), PrincipalKind::Human);
    drop(h);
    let kinds: Vec<String> = journal_headers(&dir).iter().map(|h| text_at(h, &["kind"]).unwrap()).collect();
    assert_eq!(kinds, ["principal_upgrade"]);
}

/// A script can never become human: an `mcp` port is refused at the channel even with a
/// provider that would confirm, and gets no raw path.
#[test]
fn an_mcp_port_is_never_upgraded() {
    let base = temp_dir("mcp");
    let dir = new_project(&base);
    let mut h = host(confirming_host(), PortKind::Mcp, fid_bus::policy::fs::Roots::default());
    assert_eq!(h.principal_kind(), PrincipalKind::Agent);
    let e = h.confirm_human().unwrap_err();
    assert_eq!((e.code(), e.reason()), ("upgrade_refused", "channel"));
    let e = h.open_project(ProjectRef::RawPath(dir), OpenMode::Write).unwrap_err();
    assert_eq!(e.reason(), "raw_path_refused");
    assert_eq!(h.principal_kind(), PrincipalKind::Agent);
}

/// AC-8: opened writable, the lock is held while the transport lives and released after it
/// is dropped; opened read-only, no lock is taken.
#[test]
fn ac8_the_lock_lives_as_long_as_the_transport() {
    let base = temp_dir("ac8");
    let dir = new_project(&base);
    let mut h = host(plain_host(), PortKind::Cli, roots("work", "export", &base));
    h.open_project(ProjectRef::Handle("root:work/p".into()), OpenMode::Write).unwrap();
    let t = h.into_transport();
    assert!(t.has_project());
    assert!(lock_record(&dir).contains(&std::process::id().to_string()), "{}", lock_record(&dir));
    let e = ProjectLocation::open(&dir, OpenMode::Write).unwrap_err();
    assert_eq!(e.code(), "locked");
    drop(t);
    assert_eq!(lock_record(&dir).trim(), "");
    drop(ProjectLocation::open(&dir, OpenMode::Write).unwrap());

    let mut h = host(plain_host(), PortKind::Cli, roots("work", "export", &base));
    h.open_project(ProjectRef::Handle("root:work/p".into()), OpenMode::Read).unwrap();
    let t = h.into_transport();
    assert_eq!(lock_record(&dir).trim(), "");
    drop(ProjectLocation::open(&dir, OpenMode::Write).unwrap());
    drop(t);
}

fn demo_def(major: u32, state: &str) -> CommandDef {
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

/// D-035 end to end: through the composed bus and the transport, a lifecycle-hidden major is
/// refused with the bytes of a major the table does not hold, and for an `mcp` port a
/// `never`-projected workspace major reads exactly as an unknown command.
#[test]
fn d035_hidden_commands_read_as_unknown_end_to_end() {
    let base = temp_dir("d035");
    new_project(&base);
    let mut defs: Vec<CommandDef> = fid_compose::workspace_defs().unwrap();
    defs.push(demo_def(1, "hidden"));
    defs.push(demo_def(2, "active"));
    let catalogue = fid_registry_build::workspace_catalogue().unwrap();
    let comp = Composition::new(Digest([5; 32]), defs, catalogue, HANDLER_SOURCES.to_vec(), Vec::new()).unwrap();
    let mut h = InProcessHost::with_host(plain_host(), PortKind::Cli, roots("work", "import", &base), comp).unwrap();
    h.open_project(ProjectRef::Handle("root:work/p".into()), OpenMode::Read).unwrap();
    let mut t = tracker(h.into_transport());
    let cmd = CommandId::parse("demo.thing@1").unwrap();
    let env = send(&mut t, "demo.thing@1", obj(&[]));
    assert_eq!(encoded(&env), encoded(&hidden_as_unknown_major(&cmd)));

    // An agent never sees a `never`-projected major.
    let comp = Composition::workspace().unwrap();
    let never = comp
        .majors()
        .into_iter()
        .find(|id| mcp_projection(comp.def(id).unwrap()) == Projection::Never)
        .expect("a never-projected workspace major");
    let mut h = InProcessHost::with_host(plain_host(), PortKind::Mcp, roots("work", "import", &base), comp).unwrap();
    h.open_project(ProjectRef::Handle("root:work/p".into()), OpenMode::Read).unwrap();
    let mut t = tracker(h.into_transport());
    let id = CommandId::parse(&never).unwrap();
    let env = send(&mut t, &never, obj(&[]));
    let unknown = ApiError::new(
        "not_found",
        "unknown_command",
        &format!("{:?}@{}: unknown command", id.name(), id.major()),
        ApiStage::Resolve,
    )
    .unwrap();
    assert_eq!(encoded(&env), encoded(&envelope_for(&id, &ApiReply::Failed(unknown))));
}

/// D-045 end to end: a dry run of a W2 command on a composed project bus is a `dry_run` reply
/// and envelope with no journal entry hash, and nothing is appended.
#[test]
fn d045_a_dry_run_never_looks_committed() {
    let base = temp_dir("d045");
    let dir = new_project(&base);
    let comp = Composition::workspace().unwrap();
    let (bus, guard) = comp.compose_project(OpenedProject::open(&dir, OpenMode::Write).unwrap()).unwrap().into_parts();
    let session = plain_host();
    let mut port: LocalPort =
        session.open(PortKind::Cli, SessionId::new("test/d045").unwrap(), Arc::clone(&bus)).unwrap();
    let cmd = CommandId::parse("project.create_part@1").unwrap();
    let reply = port.call(&BusCall { command: cmd.clone(), params: label(), dry_run: true });
    assert!(matches!(reply, ApiReply::DryRun { .. }), "{reply:?}");
    let env = envelope_for(&cmd, &reply);
    assert_eq!(env.outcome, Outcome::DryRun);
    assert_eq!(env.journal_entry_hash, None);

    let reply = port.call(&BusCall { command: cmd.clone(), params: label(), dry_run: false });
    assert!(matches!(reply, ApiReply::Committed { .. }), "{reply:?}");
    drop(port);
    drop(bus);
    drop(guard);
    let headers = journal_headers(&dir);
    assert_eq!(headers.len(), 1, "only the real call committed");
    assert_eq!(text_at(&headers[0], &["principal", "kind"]).as_deref(), Some("script"));
}

/// A container created in one call is in the state view of the next (the commit hook), and a
/// project reopened later folds it from the journal.
#[test]
fn containers_are_folded_from_commits_and_from_the_journal() {
    let base = temp_dir("fold");
    let dir = new_project(&base);
    let comp = Composition::workspace().unwrap();
    let (bus, guard) = comp.compose_project(OpenedProject::open(&dir, OpenMode::Write).unwrap()).unwrap().into_parts();
    let mut port = plain_host().open(PortKind::Cli, SessionId::new("test/fold").unwrap(), Arc::clone(&bus)).unwrap();
    let id = "0190a3b2-0000-7000-8000-0000000000aa";
    let part = obj(&[("label", CValue::Str("P".into())), ("id", CValue::Str(id.into()))]);
    let cmd = CommandId::parse("project.create_part@1").unwrap();
    let call = |params: CValue| BusCall { command: cmd.clone(), params, dry_run: false };
    assert!(matches!(port.call(&call(part.clone())), ApiReply::Committed { .. }));
    // The same imported ID again is a collision: the view holds the first part.
    match port.call(&call(part.clone())) {
        ApiReply::Failed(e) => assert_eq!((e.code(), e.reason()), ("conflict", "id_collision")),
        other => panic!("{other:?}"),
    }
    drop(port);
    drop(bus);
    drop(guard);

    // Reopened, the project's state is folded from its journal.
    let (bus, guard) = comp.compose_project(OpenedProject::open(&dir, OpenMode::Write).unwrap()).unwrap().into_parts();
    let mut port = plain_host().open(PortKind::Cli, SessionId::new("test/fold2").unwrap(), Arc::clone(&bus)).unwrap();
    match port.call(&call(part)) {
        ApiReply::Failed(e) => assert_eq!((e.code(), e.reason()), ("conflict", "id_collision")),
        other => panic!("{other:?}"),
    }
    drop(port);
    drop(bus);
    drop(guard);
}
