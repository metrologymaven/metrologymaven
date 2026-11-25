//! AUTO.CLI.DISPATCH AC-6, second half: the flag table of every major in the workspace
//! registry manifest builds with no `flag_collision` (REQ-AUTO.CLI.DISPATCH-003). A build-time
//! check, so a collision never reaches a user.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_cli::dispatch::check_tree_flags;
use fid_cli::tree::{emit, parse_tree};
use fid_registry_build::{build_workspace, workspace_catalogue};
use fid_surface::project;

#[test]
fn workspace_manifest_has_no_flag_collision() {
    let export = build_workspace(&workspace_catalogue().unwrap()).unwrap();
    let proj = project(&export.bytes).unwrap();
    assert!(proj.commands.len() >= 30, "expected the workspace's commands, found {}", proj.commands.len());
    let tree = parse_tree(&emit(&proj).unwrap().bytes).unwrap();
    if let Err(errs) = check_tree_flags(&tree) {
        let lines: Vec<String> = errs.iter().map(ToString::to_string).collect();
        panic!("flag collisions in the workspace manifest:\n{}", lines.join("\n"));
    }
}
