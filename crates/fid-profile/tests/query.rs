//! Acceptance and unit tests for `TRUST.PROFILE.GATE.QUERY` (`spec/trust/profile/gate/query/NODE.md`):
//! AC-1..AC-3 (AC-4 compile-fail tests live as doc-tests in `src/query.rs`), plus fail-closed
//! corrupt selection handling and tenant-parameterized Regulated gate propagation.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, Barrier};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;

use fid_profile::query::{GateSwitch, GuiGates, ProfileQuery, SelectionSlot, StoredSelection, CONFIG_KEY};
use fid_profile::{AgentWrites, MeaningCode, ProfileName, ProgramReleaseByOther};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().join("fixtures/profile")
}

fn pinned_digest(name: &str) -> String {
    let text = fs::read_to_string(fixtures_dir().join("digests.json")).unwrap();
    let v = fid_canon::parse(text.as_bytes()).unwrap();
    let fid_canon::CValue::Obj(o) = v else { panic!("digests object") };
    let fid_canon::CValue::Str(s) = o.get(name).unwrap() else { panic!("digest string for {name}") };
    s.clone()
}

/// AC-1: Given each built-in from `fixtures/profile/builtin/`, selecting it makes `active()`
/// return the matching name, digest, and all parameter accessors, and `gui_gates()` equals
/// `fixtures/profile/gates/<name>.json`.
#[test]
fn ac1_builtins_match_fixtures_and_gates_json() {
    assert_eq!(CONFIG_KEY, "trust.profile");
    let slot = SelectionSlot::unset();
    let query = slot.query();

    for (wire, name) in [
        ("lean", ProfileName::Lean),
        ("regulated", ProfileName::Regulated),
        ("assistive_only", ProfileName::AssistiveOnly),
    ] {
        let doc_bytes = fs::read(fixtures_dir().join(format!("builtin/{wire}.json"))).unwrap();
        slot.set(StoredSelection::CanonicalDocument(doc_bytes.clone())).unwrap();

        let snap = query.active().expect("active built-in");
        assert_eq!(snap.name(), name);
        assert_eq!(snap.digest().as_str(), pinned_digest(wire));
        assert_eq!(snap.canonical_bytes(), doc_bytes.as_slice());

        match name {
            ProfileName::Lean => {
                assert!(!snap.release_requires_signatures());
                assert!(snap.allow_software_keys());
                assert_eq!(snap.agent_writes(), AgentWrites::Branch);
                assert_eq!(snap.program_release_by_other(), ProgramReleaseByOther::AgentAuthored);
                assert!(snap.required_meanings().is_empty());
                assert!(snap.tenant_meanings().is_empty());
            }
            ProfileName::Regulated => {
                assert!(snap.release_requires_signatures());
                assert!(!snap.allow_software_keys());
                assert_eq!(snap.agent_writes(), AgentWrites::Branch);
                assert_eq!(snap.program_release_by_other(), ProgramReleaseByOther::Always);
                assert_eq!(
                    snap.required_meanings(),
                    &[MeaningCode::new("AUTHORED").unwrap(), MeaningCode::new("APPROVED").unwrap()]
                );
            }
            ProfileName::AssistiveOnly => {
                assert!(!snap.release_requires_signatures());
                assert!(snap.allow_software_keys());
                assert_eq!(snap.agent_writes(), AgentWrites::ProposalsOnly);
                assert_eq!(snap.program_release_by_other(), ProgramReleaseByOther::AgentAuthored);
                assert!(snap.required_meanings().is_empty());
            }
        }

        let got_gates = String::from_utf8(snap.gui_gates().to_canonical_json().unwrap()).unwrap();
        let expected_gates = fs::read_to_string(fixtures_dir().join(format!("gates/{wire}.json"))).unwrap();
        assert_eq!(got_gates, expected_gates.trim_end_matches('\n'), "gates mismatch for {wire}");
    }
}

/// AC-2: Given no selection (`Unset`), `active()` returns Lean with the pinned Lean digest.
#[test]
fn ac2_unset_selection_defaults_to_lean_with_pinned_digest() {
    let slot = SelectionSlot::unset();
    let query = slot.query();
    let snap = query.active().expect("default Lean snapshot");
    assert_eq!(snap.name(), ProfileName::Lean);
    assert_eq!(snap.digest().as_str(), pinned_digest("lean"));
    assert!(!snap.gui_gates().require_signature_before_release);
    assert_eq!(snap.gui_gates().software_key_signing, GateSwitch::Enabled);
    assert_eq!(snap.gui_gates().agent_direct_write, GateSwitch::Enabled);
}

/// AC-3: Given a snapshot held while another thread sets Regulated, the held snapshot still
/// reports Lean and a new `active()` reports Regulated.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ac3_held_snapshot_is_immutable_across_concurrent_profile_change() {
    let slot = SelectionSlot::unset();
    let query = slot.query();

    // Take Lean snapshot on main thread before concurrent update.
    let held_snap = query.active().expect("initial Lean snapshot");
    assert_eq!(held_snap.name(), ProfileName::Lean);

    let barrier = Arc::new(Barrier::new(2));
    let b_worker = Arc::clone(&barrier);
    let slot_worker = slot.clone();
    let handle = thread::spawn(move || {
        slot_worker.set(StoredSelection::Named { name: ProfileName::Regulated, tenant_file: None }).unwrap();
        b_worker.wait();
    });

    barrier.wait();
    handle.join().unwrap();

    // Held snapshot still reports Lean with unchanged digest and permissive gates.
    assert_eq!(held_snap.name(), ProfileName::Lean);
    assert_eq!(held_snap.digest().as_str(), pinned_digest("lean"));
    assert!(!held_snap.gui_gates().require_signature_before_release);
    assert_eq!(held_snap.gui_gates().software_key_signing, GateSwitch::Enabled);

    // A fresh active() call reports Regulated.
    let updated_snap = query.active().expect("updated Regulated snapshot");
    assert_eq!(updated_snap.name(), ProfileName::Regulated);
    assert_eq!(updated_snap.digest().as_str(), pinned_digest("regulated"));
    assert!(updated_snap.gui_gates().require_signature_before_release);
    assert_eq!(updated_snap.gui_gates().software_key_signing, GateSwitch::Disabled);
}

/// Corrupt stored selection fails closed: `active()` returns `profile.malformed` and
/// `gui_gates_fail_closed()` returns Regulated flags.
#[test]
fn corrupt_selection_errors_and_fails_closed_to_regulated_gates() {
    let slot = SelectionSlot::new(StoredSelection::CanonicalDocument(b"{not valid json".to_vec()));
    let query = slot.query();
    let err = query.active().expect_err("corrupt selection must fail");
    assert_eq!(err.code(), "profile.malformed");

    let gates = query.gui_gates_fail_closed();
    assert_eq!(gates, GuiGates::fail_closed_regulated());
    assert!(gates.require_signature_before_release);
    assert_eq!(gates.software_key_signing, GateSwitch::Disabled);
}

/// Regulated with tenant parameter file (`regulated-with-xqa.json`) propagates custom
/// `required_meanings` into `gui_gates()`.
#[test]
fn regulated_with_tenant_meanings_propagates_to_gates() {
    let tenant_bytes = fs::read(fixtures_dir().join("regulated-with-xqa.json")).unwrap();
    let slot =
        SelectionSlot::new(StoredSelection::Named { name: ProfileName::Regulated, tenant_file: Some(tenant_bytes) });
    let query = slot.query();
    let snap = query.active().expect("regulated with tenant meanings");
    let gates = snap.gui_gates();
    let codes: Vec<&str> = gates.required_meanings.iter().map(|m| m.as_str()).collect();
    assert_eq!(codes, vec!["AUTHORED", "X_QA_RELEASE", "APPROVED"]);
    assert_eq!(snap.tenant_meanings().len(), 1);
    assert_eq!(snap.tenant_meanings()[0].code().as_str(), "X_QA_RELEASE");
}
