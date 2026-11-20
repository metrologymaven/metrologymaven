//! The surface projection (AUTO.SURF.PROJ) over the full workspace manifest: it must succeed,
//! and for every command on MCP it must agree with the bus's own projection decision
//! (`fid_bus::policy::projection::mcp_projection`, CORE.BUS.POL.PROJ), which is the authority.
//! Asked for in the review of #348, which found `request.reject_request@1` (W1, `human_only`,
//! declared `never`) refused with `projection_mismatch`, so `cargo xtask gen-surfaces` over the
//! workspace emitted nothing. The expectations are the spec's rules, not a recorded run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::policy::projection::mcp_projection;
use fid_registry::{Projection as Declared, Surface};
use fid_registry_build::{build_workspace, workspace_catalogue, SOURCES};
use fid_surface::{project, McpProjection};

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

/// For every workspace command on the MCP surface, the surface projection's visibility equals
/// the bus's: `exposed` and `named` are the bus's `exposed`, `request_only` is
/// `request_only`, `none` is `never`. Commands without `mcp` in `exposure` are `never` on the
/// bus whatever they declare; the MCP emitter (AUTO.MCP.PROJ) applies that, so they are
/// checked only to project to something no looser than the bus's class ceiling.
#[test]
fn surface_projection_agrees_with_the_bus() {
    let export = build_workspace(&workspace_catalogue().unwrap()).unwrap();
    let p = project(&export.bytes).unwrap();
    let mut checked = 0;
    for s in SOURCES.iter() {
        for d in (s.defs)().unwrap() {
            let id = format!("{}@{}", d.id, d.major);
            let c = p.commands.iter().find(|c| c.command_id.as_str() == id).unwrap_or_else(|| panic!("{id} missing"));
            let surface = match c.mcp_projection {
                McpProjection::Exposed | McpProjection::Named => Declared::Exposed,
                McpProjection::RequestOnly => Declared::RequestOnly,
                McpProjection::None => Declared::Never,
            };
            if d.exposure.contains(&Surface::Mcp) {
                assert_eq!(surface, mcp_projection(&d), "{} ({})", id, s.krate);
                checked += 1;
            } else {
                assert!(surface <= d.effective_projection(), "{id} projects looser than it declares");
            }
        }
    }
    assert!(checked >= 20, "{checked}");
}
