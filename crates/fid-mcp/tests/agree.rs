//! The MCP projection agrees with the bus's own decision (CORE.BUS.POL.PROJ) for every command
//! of the workspace registry and for every combination of class, declared projection,
//! `human_only` and exposure; the text table, the channel and the principal kind are the bus's.
#![cfg(not(target_family = "wasm"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{json, manifest_of, projection_of, read};
use fid_bus::error::{BusError, Code};
use fid_bus::handler::ValidParams;
use fid_bus::policy::projection::{
    authorise, authorise_projection, forbidden_in_reason, is_mcp_channel, Decision, Projected, RequestBook,
    REASON_FORBIDDEN_RANGES,
};
use fid_bus::principal::{Channel, ChannelKind, Identity, OsUser, PrincipalId, SessionId, SessionKey, SessionTable};
use fid_bus::validate::Compiled;
use fid_canon::Digest;
use fid_mcp::adapter::PRINCIPAL_KIND;
use fid_mcp::text::{forbidden, FORBIDDEN_RANGES};
use fid_mcp::tools::tool_for;
use fid_mcp::{decide, emit, ToolKind, CHANNEL};
use fid_registry::{CommandDef, Projection as Declared, Surface};
use fid_surface::DeclaredProjection;
use fid_types::SideEffect;

/// The bus's decision for an MCP call of `def`, as one word.
fn bus_word(r: Result<Projected, BusError>) -> &'static str {
    match r {
        Ok(Projected::Run) => "direct",
        Ok(Projected::PendingRequest) => "request_only",
        Err(e) => {
            assert_eq!((e.code(), e.reason()), (Code::Policy, "not_on_mcp"), "{e}");
            "none"
        }
    }
}

fn mcp_word(k: Option<ToolKind>) -> &'static str {
    match k {
        Some(ToolKind::Direct) => "direct",
        Some(ToolKind::RequestOnly) => "request_only",
        None => "none",
    }
}

fn declared(def: &CommandDef) -> DeclaredProjection {
    DeclaredProjection::parse(def.mcp_projection.as_str()).unwrap()
}

/// `decide` on the registry fields of `def`.
fn mcp_decision(def: &CommandDef) -> &'static str {
    mcp_word(decide(def.side_effect, declared(def), def.human_only, def.exposure.contains(&Surface::Mcp)))
}

/// The emitter's decision for `def` when the surface projection accepts it.
fn emitted(def: &CommandDef) -> Option<&'static str> {
    let p = projection_of(def).ok()?;
    let sp = p.commands.first().unwrap();
    Some(mcp_word(tool_for(sp).unwrap_or_else(|e| panic!("{}@{}: {e}", def.id, def.major)).map(|t| t.kind)))
}

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

fn mcp_channel(table: &mut SessionTable) -> TestChannel {
    let mut identity = Identity::new(PrincipalId::new("agent-1").unwrap());
    identity.os_user = Some(OsUser::new("agent-1").unwrap());
    let session = table.open_session(SessionId::new("mcp-agent-1").unwrap()).unwrap();
    TestChannel { kind: ChannelKind::from_wire(CHANNEL).unwrap(), identity, session }
}

/// The full authorise stage for an agent on the MCP channel, as one word.
fn authorise_word(table: &SessionTable, ch: &TestChannel, def: &CommandDef) -> &'static str {
    let book = RequestBook::new();
    let view = book.view(Digest([0x77; 32]));
    let params = ValidParams::for_tests(json("{}"));
    match authorise(table, ch, def, &params, None, &view) {
        Ok(a) => {
            assert_eq!(a.principal().kind().as_str(), PRINCIPAL_KIND);
            match a.decision() {
                Decision::Run => "direct",
                Decision::File(_) => "request_only",
                Decision::RunApproved(_) => panic!("no approval was asked for"),
            }
        }
        Err(e) => {
            assert_eq!((e.code(), e.reason()), (Code::Policy, "not_on_mcp"), "{e}");
            "none"
        }
    }
}

/// A template definition with no parameters (min-classes `align.adopt_alignment@1`).
fn template() -> CommandDef {
    let defs = common::defs_of(&read("fixtures/manifest/min-classes.json"));
    defs.into_iter().find(|d| d.id == "align.adopt_alignment").unwrap()
}

/// Every class x declared projection x human_only x exposure: the MCP decision equals the
/// bus's projection check and its full authorise stage for an agent, and the emitter agrees
/// wherever the surface projection accepts the definition.
#[test]
fn every_class_combination_agrees_with_the_bus() {
    let mut table = SessionTable::new();
    let ch = mcp_channel(&mut table);
    let mut checked = 0;
    let mut emitted_checked = 0;
    for class in SideEffect::ALL.iter().copied() {
        for proj in [Declared::Never, Declared::RequestOnly, Declared::Exposed] {
            for human_only in [false, true] {
                for on_mcp in [false, true] {
                    let mut def = template();
                    def.side_effect = class;
                    def.mcp_projection = proj;
                    def.human_only = human_only;
                    let surfaces = if on_mcp { [Surface::Cli, Surface::Mcp] } else { [Surface::Cli, Surface::Python] };
                    def.exposure = surfaces.into_iter().collect();
                    let case = format!("{class:?} {proj:?} human_only={human_only} mcp={on_mcp}");
                    let bus = bus_word(authorise_projection(ChannelKind::Mcp, &def));
                    assert_eq!(mcp_decision(&def), bus, "{case}");
                    assert_eq!(authorise_word(&table, &ch, &def), bus, "{case}: full authorise");
                    if let Some(e) = emitted(&def) {
                        assert_eq!(e, bus, "{case}: emitter");
                        emitted_checked += 1;
                    }
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 11 * 3 * 2 * 2);
    // Every class has at least one consistent declaration the emitter sees.
    assert!(emitted_checked >= 11 * 2, "{emitted_checked}");
}

/// Every command of the workspace registry: the MCP decision equals the bus's, and the
/// emitter builds a tool from the command's real parameter and result schemas.
#[test]
fn whole_registry_agrees_with_the_bus() {
    // Every declared command: SOURCES plus egress, RDL, GDT (3), the package reader, report
    // binding and the two export commands.
    let all = fid_registry_build::test_support::all_defs().unwrap();
    let krates: std::collections::BTreeSet<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
    for k in ["fid-bus (egress)", "fid-rdl", "fid-gdt", "fid-pkg", "fid-report (bind)", "fid-io-reg"] {
        assert!(krates.contains(k), "{k} is missing from the walk");
    }
    let defs: Vec<CommandDef> = all.into_iter().map(|(_, d)| d).collect();
    assert!(defs.len() >= 30, "{}", defs.len());
    let mut table = SessionTable::new();
    let ch = mcp_channel(&mut table);
    let mut refused_by_surface = Vec::new();
    for def in &defs {
        let id = format!("{}@{}", def.id, def.major);
        let bus = bus_word(authorise_projection(ChannelKind::Mcp, def));
        assert_eq!(mcp_decision(def), bus, "{id}");
        match projection_of(def) {
            Ok(p) => {
                let sp = p.commands.first().unwrap();
                let tool = tool_for(sp).unwrap_or_else(|e| panic!("{id}: {e}"));
                assert_eq!(mcp_word(tool.as_ref().map(|t| t.kind)), bus, "{id}: emitter");
                if let Some(t) = tool {
                    assert_eq!(t.name, sp.mcp_name, "{id}");
                    // The bus's own validator compiles both generated schemas.
                    for (what, schema) in [("input", &t.input_schema), ("output", &t.output_schema)] {
                        if let Err(e) = Compiled::compile(schema) {
                            panic!("{id}: {what} schema does not compile for the bus: {e}");
                        }
                    }
                }
            }
            Err(e) => {
                // AUTO.SURF.PROJ refuses a declaration that differs from the class table even
                // when it is stricter; the bus takes the stricter one. Generation then fails as
                // a whole, so nothing is listed: fail closed. It must never be a command the
                // bus would run for an agent.
                assert_eq!(e.code(), "projection_mismatch", "{id}: {e}");
                assert_ne!(bus, "direct", "{id}");
                refused_by_surface.push(id.clone());
            }
        }
        // Filing re-checks the parameters against the schema, so the empty-parameter call is
        // made only where no filing happens.
        if bus != "request_only" {
            assert_eq!(authorise_word(&table, &ch, def), bus, "{id}: full authorise");
        }
    }
    // The whole workspace manifest: emitted when the surface projection accepts it.
    let m = manifest_of(&defs);
    if let Ok(p) = fid_surface::project(&m) {
        let out = emit(&p).unwrap();
        for def in &defs {
            let listed = out.tools.iter().find(|t| t.command.name() == def.id && t.command.major() == def.major);
            assert_eq!(mcp_word(listed.map(|t| t.kind)), mcp_decision(def), "{}@{}", def.id, def.major);
        }
    } else {
        assert!(!refused_by_surface.is_empty());
    }
}

/// The text table is the bus's rejection-reason table, entry for entry, and the two
/// predicates agree on every Unicode scalar value.
#[test]
fn forbidden_table_is_the_bus_table() {
    assert_eq!(FORBIDDEN_RANGES, REASON_FORBIDDEN_RANGES);
    for u in 0..=0x10_FFFFu32 {
        if let Some(c) = char::from_u32(u) {
            assert_eq!(forbidden(c), forbidden_in_reason(c), "U+{u:04X}");
        }
    }
}

/// The adapter's channel is the bus's MCP channel, and its principal kind is the one the bus
/// derives there.
#[test]
fn channel_and_principal_kind_are_the_bus_ones() {
    let kind = ChannelKind::from_wire(CHANNEL).unwrap();
    assert_eq!(kind, ChannelKind::Mcp);
    assert!(is_mcp_channel(kind));
    assert_eq!(kind.base_kind().as_str(), PRINCIPAL_KIND);
}
