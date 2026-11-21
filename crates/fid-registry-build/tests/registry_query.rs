//! The registry query commands of CORE.REG.QRY against the bus (`fid_bus`), which
//! `fid-registry` cannot depend on:
//!
//! - their parameter and result schemas validate real documents with the bus's own validator
//!   (`fid_bus::validate::Compiled`, CORE.BUS.PIPE.VALID), and refuse unknown keys;
//! - `manifest.schema.json` (compiled by the same validator) accepts exactly what
//!   `query::read_manifest` takes for its schema step;
//! - `query::mcp_projection` equals `fid_bus::policy::projection::mcp_projection` on every
//!   declared command, so an agent's view of the workspace registry hides exactly the commands
//!   the bus refuses on MCP (D-035), including every `human_only` command it never projects;
//! - AC-6: an agent's raw path to `registry_diff_manifests` is refused by the bus, a handle
//!   passes and the diff runs.
//!
//! Expected results come from `fixtures/core/reg/qry/make_cases.py` (independent Python) or are
//! the spec's rules, never a recorded run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code, Detail, ValidationReason};
use fid_bus::policy::fs::{check_raw_paths, path_like_params, FileHandle};
use fid_bus::policy::projection::{authorise_projection, mcp_projection as bus_projection, Projected};
use fid_bus::principal::{
    Channel, ChannelKind, Identity, OsUser, Principal, PrincipalId, PrincipalKind, SessionId, SessionKey, SessionTable,
};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::manifest::manifest_schema;
use fid_registry::query::{self, invoke_mcp, read_manifest, Audience, Registry, MCP_DIFF};
use fid_registry::{CommandDef, Projection};
use fid_registry_build::{manifest_in_force, test_support, workspace_catalogue};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/reg")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(rel)).unwrap()
}

fn j(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no literals")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn passes(schema: &CValue, v: &CValue) -> bool {
    Compiled::compile(schema).unwrap().evaluate(v, &NoHooks).is_ok()
}

fn get<'v>(v: &'v CValue, k: &str) -> &'v CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing {k}")),
        _ => panic!("not an object"),
    }
}

fn defs() -> BTreeMap<String, CommandDef> {
    query::command_defs().unwrap().into_iter().map(|d| (d.id.clone(), d)).collect()
}

// ------------------------------------------------------------------ schemas

#[test]
fn query_schemas_validate_with_the_bus_validator() {
    let d = defs();
    let list = &d["registry.list_commands"];
    let describe = &d["registry.describe_command"];
    let diff = &d["registry.diff_manifests"];
    for def in d.values() {
        Compiled::compile(&def.params_schema).unwrap();
        Compiled::compile(&def.result_schema).unwrap();
    }
    // Parameters: every oracle case's verdict on shape (the oracle's bad_params cases that are
    // about shape, not about the 256-byte ID or a major beyond 2^32, also fail the schema).
    for ok in [
        "{}",
        r#"{"domain":"align","side_effect":"W2","lifecycle":"hidden","include_hidden":true}"#,
        r#"{"side_effect":"A0"}"#,
    ] {
        assert!(passes(&list.params_schema, &j(ok)), "{ok}");
    }
    for bad in [
        r#"{"domains":"align"}"#,
        r#"{"domain":5}"#,
        r#"{"domain":""}"#,
        r#"{"side_effect":"W9"}"#,
        r#"{"lifecycle":"gone"}"#,
        r#"{"include_hidden":"yes"}"#,
        r#"["domain"]"#,
    ] {
        assert!(!passes(&list.params_schema, &j(bad)), "{bad}");
    }
    let long_domain = format!("{{\"domain\":\"{}\"}}", "a".repeat(65));
    assert!(!passes(&list.params_schema, &j(&long_domain)));
    for ok in [r#"{"id":"align.run_bestfit"}"#, r#"{"id":"align.run_bestfit","major":2}"#] {
        assert!(passes(&describe.params_schema, &j(ok)), "{ok}");
    }
    for bad in [
        r#"{"major":1}"#,
        r#"{"id":7}"#,
        r#"{"id":"cmm.move_to","major":-1}"#,
        r#"{"id":"cmm.move_to","major":1.5}"#,
        r#"{"id":"cmm.move_to","major":4294967296}"#,
        r#"{"id":"cmm.move_to","verbose":true}"#,
    ] {
        assert!(!passes(&describe.params_schema, &j(bad)), "{bad}");
    }
    assert!(passes(&diff.params_schema, &j(r#"{"a":"root:imports/v1.json","b":"root:imports/v2.json"}"#)));
    for bad in [r#"{"a":"root:imports/v1.json"}"#, r#"{"a":"x","b":"y","c":"z"}"#, r#"{"a":1,"b":"y"}"#] {
        assert!(!passes(&diff.params_schema, &j(bad)), "{bad}");
    }
    assert_eq!(path_like_params(diff), vec!["a".to_string(), "b".to_string()]);

    // Results: every expected result of the oracle passes its result schema.
    let cases = parse(&read("qry/cases.json")).unwrap();
    let CValue::Arr(cases) = get(&cases, "cases") else { panic!() };
    let mut checked = 0;
    for case in cases {
        let CValue::Obj(expect) = get(case, "expect") else { panic!() };
        let Some(ok) = expect.get("ok") else { continue };
        let schema = match get(case, "tool") {
            CValue::Str(t) if t == "list" => &list.result_schema,
            _ => &describe.result_schema,
        };
        assert!(passes(schema, ok), "{:?}", get(case, "name"));
        checked += 1;
    }
    assert!(checked >= 20, "{checked}");
    assert!(passes(&diff.result_schema, &parse(&read("qry/v1_v2.changes.json")).unwrap()));
    assert!(!passes(
        &diff.result_schema,
        &j(r#"{"changes":[{"id":"a","major":1,"kind":"moved","compat":"additive","fields":[]}]}"#)
    ));
}

#[test]
fn manifest_schema_agrees_with_the_query_reader() {
    let schema = manifest_schema();
    Compiled::compile(&schema).unwrap();
    for ok in ["manifest/v1.json", "manifest/v2.json", "manifest/valid.manifest.json"] {
        let bytes = read(ok);
        assert!(passes(&schema, &parse(&bytes).unwrap()), "{ok}");
        assert!(read_manifest(&bytes).is_ok(), "{ok}");
    }
    // tampered.json matches the schema; only its digest is wrong.
    let tampered = read("manifest/tampered.json");
    assert!(passes(&schema, &parse(&tampered).unwrap()));
    assert_eq!(read_manifest(&tampered).unwrap_err().code(), "manifest_invalid");
    // Shape errors fail both.
    let CValue::Obj(mut v1) = parse(&read("manifest/v1.json")).unwrap() else { panic!() };
    let Some(CValue::Arr(items)) = v1.get_mut("commands") else { panic!() };
    let Some(CValue::Obj(first)) = items.first_mut() else { panic!() };
    first.remove("cli_bare");
    let shape = CValue::Obj(v1);
    assert!(!passes(&schema, &shape));
    assert_eq!(read_manifest(&to_canonical_bytes(&shape).unwrap()).unwrap_err().code(), "manifest_invalid");
}

// ------------------------------------------------------------------ projection

#[test]
fn mcp_projection_agrees_with_the_bus_on_every_declared_command() {
    let all = test_support::all_defs().unwrap();
    assert!(all.len() >= 30);
    for (krate, def) in &all {
        assert_eq!(query::mcp_projection(def), bus_projection(def), "{krate} {}@{}", def.id, def.major);
    }
    // Every human_only command of the workspace that MCP never projects is invisible to an
    // agent, and the rest appear at most as request_only.
    let human_only: Vec<&CommandDef> = all.iter().map(|(_, d)| d).filter(|d| d.human_only).collect();
    assert!(!human_only.is_empty());
    for d in human_only {
        assert!(query::mcp_projection(d) <= Projection::RequestOnly, "{}@{}", d.id, d.major);
    }
}

#[test]
fn an_agent_sees_exactly_the_workspace_commands_the_bus_runs_or_files() {
    let m = manifest_in_force(workspace_catalogue().unwrap()).unwrap();
    let r = Registry::new(&m).unwrap();
    let all =
        query::list_commands(&r, Audience::Person, &query::ListParams { include_hidden: true, ..Default::default() });
    let agent =
        query::list_commands(&r, Audience::Agent, &query::ListParams { include_hidden: true, ..Default::default() });
    let ids = |v: &CValue| -> BTreeSet<(String, String)> {
        let CValue::Arr(c) = get(v, "commands") else { panic!() };
        c.iter()
            .map(|s| {
                let (CValue::Str(id), CValue::Str(p)) = (get(s, "id"), get(s, "mcp_projection")) else { panic!() };
                (id.clone(), p.clone())
            })
            .collect()
    };
    let (all, agent) = (ids(&all), ids(&agent));
    assert!(all.len() >= 30);
    let expected: BTreeSet<_> = all.iter().filter(|(_, p)| p != "never").cloned().collect();
    assert_eq!(agent, expected);
    assert!(!agent.iter().any(|(id, _)| id == "request.reject_request"), "never-projected human_only command leaked");
    for id in ["registry.list_commands", "registry.describe_command", "registry.diff_manifests"] {
        assert!(agent.contains(&(id.to_string(), "exposed".to_string())), "{id}");
    }
}

// ------------------------------------------------------------------ AC-6

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

fn agent() -> Principal {
    let mut table = SessionTable::new();
    let mut identity = Identity::new(PrincipalId::new("agent-1").unwrap());
    identity.os_user = Some(OsUser::new("agent-1").unwrap());
    let session = table.open_session(SessionId::new("mcp-agent-1").unwrap()).unwrap();
    let ch = TestChannel { kind: ChannelKind::Mcp, identity, session };
    let p = table.derive_principal(&ch).unwrap();
    assert_eq!(p.kind(), PrincipalKind::Agent);
    p
}

#[test]
fn ac6_agent_raw_path_is_refused_by_the_bus_and_a_handle_succeeds() {
    let def = &defs()["registry.diff_manifests"];
    let agent = agent();
    assert_eq!(authorise_projection(ChannelKind::Mcp, def).unwrap(), Projected::Run);
    let raw = j(r#"{"a":"/home/me/v1.json","b":"root:imports/v2.json"}"#);
    let e = check_raw_paths(&agent, def, &raw).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Policy, "raw_path_refused"), "{e}");
    let handles = j(r#"{"a":"root:imports/v1.json","b":"root:imports/v2.json"}"#);
    check_raw_paths(&agent, def, &handles).unwrap();
    for h in ["root:imports/v1.json", "root:imports/v2.json"] {
        FileHandle::parse(h).unwrap();
    }
    // The bus resolves each handle below the approved import root (CORE.BUS.POL.FS) and hands
    // the bytes to the command; here the resolver maps the two handles to the fixtures.
    let m = manifest_in_force(workspace_catalogue().unwrap()).unwrap();
    let r = Registry::new(&m).unwrap();
    let files = |h: &str| match h {
        "root:imports/v1.json" => Some(read("manifest/v1.json")),
        "root:imports/v2.json" => Some(read("manifest/v2.json")),
        _ => None,
    };
    let mut log = Vec::new();
    let out = invoke_mcp(&r, &mut log, MCP_DIFF, &to_canonical_bytes(&handles).unwrap(), &files).unwrap();
    assert_eq!(parse(&out).unwrap(), parse(&read("qry/v1_v2.changes.json")).unwrap());
    // Defence in depth: the command itself refuses a raw path from an agent.
    let err = invoke_mcp(&r, &mut log, MCP_DIFF, &to_canonical_bytes(&raw).unwrap(), &files).unwrap_err();
    assert_eq!(err.code(), "bad_params");
}
