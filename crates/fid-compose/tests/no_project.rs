//! AC-4 of spec node AUTO.SURF.COMPOSE: the no-project bus. Its own test binary, because it
//! runs in a fresh temporary working directory and checks that nothing in it changes.
//!
//! Every manifest major that no handler source registers without a project is sent from a
//! `cli` port with the smallest parameter document its schema accepts. Each finishes
//! `failed`. A call that reaches the handler stage gets the no-project stub:
//! `precondition_failed` with detail `no project open`. The bus's own stages refuse some
//! majors before any handler can run (for example `policy/class_forbidden` for a W3 command
//! from a script, D-035 for a hidden major, a parameter the generator could not satisfy);
//! those are listed, and none of them may succeed or write anything. Hand-labelled anchors:
//! `project.list_elements@1` and `project.create_part@1` reach the stub.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_bus_api::PortKind;
use fid_compose::{Composition, NO_PROJECT_OPEN};
use fid_surface::{ErrorCode, Outcome};
use std::collections::BTreeMap;
use std::path::Path;

fn listing(dir: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        out.push(p.to_string_lossy().to_string());
        if p.is_dir() {
            listing(&p, out);
        }
    }
}

#[test]
fn ac4_the_no_project_bus_runs_nothing() {
    let cwd = temp_dir("ac4-cwd");
    std::env::set_current_dir(&cwd).unwrap();
    let mut before = Vec::new();
    listing(&cwd, &mut before);

    let comp = Composition::workspace().unwrap();
    let free = comp.wired_free().unwrap();
    let majors: Vec<(String, fid_canon::CValue)> = comp
        .majors()
        .into_iter()
        .filter(|id| !free.contains(id))
        .map(|id| {
            let params = smallest(&comp.def(&id).unwrap().params_schema);
            (id, params)
        })
        .collect();
    assert!(!majors.is_empty());
    let h = fid_compose::InProcessHost::with_host(plain_host(), PortKind::Cli, Default::default(), comp).unwrap();
    let mut t = tracker(h.into_transport());

    let mut stubbed = Vec::new();
    let mut refused_before: BTreeMap<String, String> = BTreeMap::new();
    for (id, params) in &majors {
        let env = send(&mut t, id, params.clone());
        assert_eq!(env.outcome, Outcome::Failed, "{id}: {env:?}");
        assert_eq!(env.journal_entry_hash, None, "{id}");
        let err = env.error.clone().unwrap();
        let origin = detail(&env, "bus_origin").unwrap_or_default();
        if origin == "handler" {
            assert_eq!(err.code, ErrorCode::PreconditionFailed, "{id}: {env:?}");
            assert_eq!(err.message, NO_PROJECT_OPEN, "{id}");
            assert_eq!(detail(&env, "bus_reason").as_deref(), Some("false"), "{id}");
            stubbed.push(id.clone());
        } else {
            let why = format!(
                "{} {}/{}",
                origin,
                detail(&env, "bus_code").unwrap_or_default(),
                detail(&env, "bus_reason").unwrap_or_default()
            );
            refused_before.insert(id.clone(), why);
        }
    }
    eprintln!(
        "{} majors reached the no-project stub; refused before the handler stage: {refused_before:#?}",
        stubbed.len()
    );
    for anchor in ["project.list_elements@1", "project.create_part@1"] {
        assert!(stubbed.iter().any(|s| s == anchor), "{anchor}: {:?}", refused_before.get(anchor));
    }
    // A refusal before the handler is a policy, resolve or validation refusal, never a
    // precondition with the stub's detail and never a success.
    for (id, why) in &refused_before {
        assert!(!why.starts_with("handler"), "{id}: {why}");
    }

    let mut after = Vec::new();
    listing(&cwd, &mut after);
    assert_eq!(before, after, "the working directory changed");
}
