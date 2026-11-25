//! AUTO.CLI.DISPATCH AC-6, second half: the flag tables of the workspace registry manifest
//! (REQ-AUTO.CLI.DISPATCH-003).
//!
//! AC-6 expects no collision, but the manifest on main has properties named `major`,
//! `version`, `project`, `params` and `dry_run`, whose flags equal reserved flags. Renaming them
//! changes other domains' command contracts, so this leaf does not do it. The conflict is
//! recorded under the leaf's Open questions, and this test is a ratchet: it pins exactly the
//! known collisions, so any new one fails, and so does removing one without updating the list.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_cli::dispatch::check_tree_flags;
use fid_cli::tree::{emit, parse_tree};
use fid_registry_build::{build_workspace, workspace_catalogue};
use fid_surface::project;

/// The collisions on main when this test was written, as (command major, property).
const KNOWN: [(&str, &str); 9] = [
    ("registry.describe_command@1", "major"),
    ("report.apply_template@1", "version"),
    ("report.create_report@1", "project"),
    ("report.show_template@1", "version"),
    ("request.file_request@1", "params"),
    ("sign.set_profile@1", "params"),
    ("unc.bind_instrument_model@1", "dry_run"),
    ("unc.create_model@1", "dry_run"),
    ("unc.import_instrument_model@1", "dry_run"),
];

#[test]
fn workspace_manifest_has_only_the_known_flag_collisions() {
    let export = build_workspace(&workspace_catalogue().unwrap()).unwrap();
    let proj = project(&export.bytes).unwrap();
    assert!(proj.commands.len() >= 30, "expected the workspace's commands, found {}", proj.commands.len());
    let tree = parse_tree(&emit(&proj).unwrap().bytes).unwrap();
    let mut got: Vec<(String, String)> = match check_tree_flags(&tree) {
        Ok(()) => Vec::new(),
        Err(errs) => errs.iter().map(|e| (e.major.as_str().to_string(), e.property.clone())).collect(),
    };
    got.sort();
    let want: Vec<(String, String)> = KNOWN.iter().map(|(m, p)| ((*m).to_string(), (*p).to_string())).collect();
    assert_eq!(
        got, want,
        "flag collisions in the workspace manifest changed; update KNOWN and the leaf's Open questions"
    );
}
