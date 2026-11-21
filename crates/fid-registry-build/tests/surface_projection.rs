//! The surface projection (AUTO.SURF.PROJ) over the full workspace manifest: it must succeed,
//! and for every command on MCP it must agree with the bus's own projection decision
//! (`fid_bus::policy::projection::mcp_projection`, CORE.BUS.POL.PROJ), which is the authority.
//! Asked for in the review of #348, which found `request.reject_request@1` (W1, `human_only`,
//! declared `never`) refused with `projection_mismatch`, so `cargo xtask gen-surfaces` over the
//! workspace emitted nothing. The expectations are the spec's rules, not a recorded run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::policy::projection::mcp_projection;
use fid_canon::{digest, to_canonical_bytes, CValue};
use fid_registry::{CommandDef, Projection as Declared, Surface};
use fid_registry_build::{build_workspace, test_support, workspace_catalogue};
use fid_surface::{project, McpProjection};
use std::collections::BTreeMap;

/// The canonical manifest (CORE.REG.MAN shape) of `defs`: each definition with its
/// `definition_digest`, under the manifest envelope.
fn manifest_of(defs: &[CommandDef]) -> Vec<u8> {
    let commands = defs
        .iter()
        .map(|d| {
            let v = d.to_value();
            let CValue::Obj(mut o) = v.clone() else { panic!("definition must be an object") };
            o.insert("definition_digest".into(), CValue::Str(digest(&v).unwrap().to_string()));
            CValue::Obj(o)
        })
        .collect();
    let mut m = BTreeMap::new();
    m.insert("format".to_string(), CValue::Num(1.0));
    m.insert("quantity_catalogue_digest".to_string(), CValue::Str(format!("sha256:{}", "ab".repeat(32))));
    m.insert("commands".to_string(), CValue::Arr(commands));
    to_canonical_bytes(&CValue::Obj(m)).unwrap()
}

/// The surface projection accepts the whole workspace manifest.
#[test]
fn workspace_manifest_projects() {
    let export = build_workspace(&workspace_catalogue().unwrap()).unwrap();
    let p = project(&export.bytes).unwrap_or_else(|e| panic!("surface projection refused the workspace: {e}"));
    assert!(p.commands.len() >= 30, "expected the workspace's commands, found {}", p.commands.len());
    assert!(p.commands.iter().any(|c| c.mcp_projection != McpProjection::None), "no command reaches MCP");
    let find = |id: &str| p.commands.iter().find(|c| c.command_id.as_str() == id).unwrap();
    assert_eq!(find("request.reject_request@1").mcp_projection, McpProjection::None);
    assert_eq!(find("request.file_request@1").mcp_projection, McpProjection::None);
    assert_eq!(find("request.list_requests@1").mcp_projection, McpProjection::Exposed);
    assert_eq!(find("request.withdraw_request@1").mcp_projection, McpProjection::Named);
}

/// For every declared command of the workspace (`test_support::all_defs`: `SOURCES` plus the
/// crates not in it yet) on the MCP surface, the surface projection's visibility equals
/// the bus's: `exposed` and `named` are the bus's `exposed`, `request_only` is
/// `request_only`, `none` is `never`. Commands without `mcp` in `exposure` are `never` on the
/// bus whatever they declare; the MCP emitter (AUTO.MCP.PROJ) applies that, so they are
/// checked only to project to something no looser than the bus's class ceiling.
#[test]
fn surface_projection_agrees_with_the_bus() {
    let all = test_support::all_defs().unwrap();
    let defs: Vec<CommandDef> = all.iter().map(|(_, d)| d.clone()).collect();
    let p = project(&manifest_of(&defs))
        .unwrap_or_else(|e| panic!("surface projection refused the declared commands: {e}"));
    let mut checked = 0;
    for (krate, d) in &all {
        let id = format!("{}@{}", d.id, d.major);
        let c = p.commands.iter().find(|c| c.command_id.as_str() == id).unwrap_or_else(|| panic!("{id} missing"));
        let surface = match c.mcp_projection {
            McpProjection::Exposed | McpProjection::Named => Declared::Exposed,
            McpProjection::RequestOnly => Declared::RequestOnly,
            McpProjection::None => Declared::Never,
        };
        if d.exposure.contains(&Surface::Mcp) {
            assert_eq!(surface, mcp_projection(d), "{id} ({krate})");
            checked += 1;
        } else {
            assert!(surface <= d.effective_projection(), "{id} projects looser than it declares");
        }
    }
    assert!(checked >= 20, "{checked}");
    for k in ["fid-bus (egress)", "fid-rdl", "fid-gdt", "fid-pkg", "fid-report (bind)", "fid-io-reg"] {
        assert!(all.iter().any(|(c, _)| c == k), "{k} is missing from the walk");
    }
    // The import commands (IO.REG.ENV.IMPORT) project: their domain `import` is a Python
    // keyword, escaped to module `fiducial.import_` (AUTO.SURF.PROJ REQ-004).
    for id in ["import.file@1", "import.fid_test_points@1"] {
        let c = p.commands.iter().find(|c| c.command_id.as_str() == id).unwrap_or_else(|| panic!("{id} missing"));
        assert_eq!(c.py_module, "fiducial.import_", "{id}");
        assert_eq!(c.cli_path[1], "import", "{id}");
    }
}
