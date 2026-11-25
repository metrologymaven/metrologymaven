//! AC-1 and AC-2 of spec node AUTO.SURF.COMPOSE: the handler sources mirror the registry
//! sources, and every manifest major has exactly one handler or one `UNWIRED` entry.
//! Expected values are hand-labelled from the leaf.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, Digest};
use fid_compose::{coverage, unwired_list, Composition, HANDLER_SOURCES, UNWIRED};
use fid_registry::CommandDef;
use std::collections::{BTreeMap, BTreeSet};

fn demo_def(id: &str, major: u32) -> CommandDef {
    let json = format!(
        r#"{{"id":"{id}","major":{major},"title":"Demo","description":"A test-only command.",
"params_schema":{{"type":"object","additionalProperties":false,"properties":{{}}}},
"result_schema":{{"type":"object"}},"side_effect":"R0","mcp_projection":"exposed","determinism":"D0",
"idempotency":"none","fs_effect":"none","undo":"not_undoable","preconditions":[],"effects":[],
"algorithms":[],"exposure":["cli","gui","mcp"],"human_only":false,"network":"none","cli_bare":false,
"lifecycle":{{"state":"active","since":"2026-11-25"}},"web":"available"}}"#
    );
    CommandDef::from_value(parse(json.as_bytes()).unwrap()).unwrap()
}

fn workspace_defs() -> Vec<CommandDef> {
    fid_compose::workspace_defs().unwrap()
}

fn composition(defs: Vec<CommandDef>, unwired: Vec<(String, String)>) -> Composition {
    let catalogue = fid_registry_build::workspace_catalogue().unwrap();
    Composition::new(Digest([9; 32]), defs, catalogue, HANDLER_SOURCES.to_vec(), unwired).unwrap()
}

/// AC-1: `HANDLER_SOURCES` and `fid_registry_build::SOURCES` name the same crates in the same
/// order.
#[test]
fn ac1_handler_sources_mirror_registry_sources() {
    let ours: Vec<&str> = HANDLER_SOURCES.iter().map(|s| s.krate).collect();
    let registry: Vec<&str> = fid_registry_build::SOURCES.iter().map(|s| s.krate).collect();
    assert_eq!(ours, registry);
}

/// AC-2: every major of the workspace manifest has exactly one handler or one `UNWIRED`
/// entry, and no `UNWIRED` entry has a handler. On failure the test prints the `UNWIRED` list
/// the manifest needs.
#[test]
fn ac2_every_major_is_wired_or_listed() {
    let comp = Composition::workspace().unwrap();
    let findings = comp.coverage().unwrap();
    if !findings.is_empty() {
        let wired = comp.wired().unwrap();
        let mut krate = BTreeMap::new();
        for s in &fid_registry_build::SOURCES {
            for d in (s.defs)().unwrap() {
                krate.insert(format!("{}@{}", d.id, d.major), s.krate);
            }
        }
        eprintln!("UNWIRED the manifest needs:");
        for id in comp.majors().iter().filter(|id| !wired.contains(*id)) {
            eprintln!("    (\"{id}\", \"no handler in {}\"),", krate.get(id).copied().unwrap_or("?"));
        }
    }
    assert!(findings.is_empty(), "{findings:#?}");
    // The first wired handlers of the leaf's Design.
    let wired = comp.wired().unwrap();
    for id in ["project.list_elements@1", "project.describe_element@1"] {
        assert!(wired.contains(id), "{id} is not wired");
    }
    // UNWIRED is sorted, has a reason for each entry, and every reason has one of the two forms.
    for (id, reason) in UNWIRED {
        assert!(
            reason.starts_with("no handler in ") || (reason.starts_with("no ") && reason.ends_with(" adapter yet")),
            "{id}: {reason}"
        );
    }
    let ids: Vec<&str> = UNWIRED.iter().map(|(id, _)| *id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(ids, sorted);
}

/// AC-2 (error path): a test-only source adds `demo.unwired@1` with no handler and no
/// `UNWIRED` entry; the coverage check fails naming it.
#[test]
fn ac2_a_major_with_no_handler_and_no_entry_fails() {
    let mut defs = workspace_defs();
    defs.push(demo_def("demo.unwired", 1));
    let findings = composition(defs, unwired_list()).coverage().unwrap();
    assert_eq!(findings, vec!["demo.unwired@1: no handler and no UNWIRED entry".to_string()]);
}

/// AC-2 (error path): an `UNWIRED` entry for `project.list_elements@1`, which has a handler;
/// the coverage check fails naming it.
#[test]
fn ac2_an_unwired_entry_with_a_handler_fails() {
    let mut unwired = unwired_list();
    unwired.push(("project.list_elements@1".to_string(), "no handler in fid-project-cmd".to_string()));
    unwired.sort();
    let comp = composition(workspace_defs(), unwired);
    let findings = comp.coverage().unwrap();
    assert_eq!(findings, vec!["project.list_elements@1: UNWIRED entry has a handler".to_string()]);
    // Composing that project bus fails closed too: two handlers for one major.
    let parts = fid_compose::ProjectParts {
        namespace: fid_project::Uuid::from_u128(0x0190_0000_0000_4000_8000_0000_0000_0002),
        ids: std::sync::Arc::new(std::sync::Mutex::new(fid_project::id::SystemSource)),
        branch: "main".to_string(),
    };
    let err = comp.project_handlers(&parts).unwrap_err();
    assert_eq!((err.code().as_str(), err.reason()), ("internal", "invariant_violated"));
}

/// The coverage function on hand-made sets: an entry outside the manifest, a duplicate and an
/// unsorted list are named.
#[test]
fn coverage_names_each_bad_entry() {
    let majors: BTreeSet<String> = ["a.b@1", "a.c@1"].iter().map(|s| (*s).to_string()).collect();
    let handled: BTreeSet<String> = ["a.b@1"].iter().map(|s| (*s).to_string()).collect();
    let e = |id: &str| (id.to_string(), "no handler in x".to_string());
    assert!(coverage(&majors, &handled, &[e("a.c@1")]).is_empty());
    assert_eq!(
        coverage(&majors, &handled, &[e("a.c@1"), e("z.z@1")]),
        vec!["z.z@1: UNWIRED entry is not a major of the manifest in force".to_string()]
    );
    assert_eq!(
        coverage(&majors, &handled, &[e("a.c@1"), e("a.c@1")]),
        vec!["a.c@1: UNWIRED is not sorted or lists it twice".to_string()]
    );
    assert_eq!(coverage(&majors, &handled, &[]), vec!["a.c@1: no handler and no UNWIRED entry".to_string()]);
}
