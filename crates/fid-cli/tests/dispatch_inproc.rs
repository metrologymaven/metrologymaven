//! AUTO.SURF.INPROC AC-3 (second half) against the real `AUTO.CLI.DISPATCH` absence check:
//! envelopes built by `fid_surface::inproc::envelope_for` for bus failures, checked with
//! `fid_cli::dispatch::is_absent_answer`. Expected verdicts are hand-labelled from
//! REQ-AUTO.CLI.DISPATCH-012 and REQ-AUTO.SURF.INPROC-006.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus_api::{ApiError, ApiReply, ApiStage};
use fid_cli::dispatch::is_absent_answer;
use fid_surface::inproc::envelope_for;
use fid_types::CommandId;

fn verdict(code: &str, reason: &str, stage: ApiStage) -> bool {
    let cmd = CommandId::parse("demo.show_thing@1").unwrap();
    let e = ApiError::new(code, reason, "demo.show_thing@1", stage).unwrap();
    is_absent_answer(&envelope_for(&cmd, &ApiReply::Failed(e)))
}

#[test]
fn handler_unknown_command_is_not_an_absent_answer() {
    assert!(!verdict("not_found", "unknown_command", ApiStage::Handler));
}

#[test]
fn pipeline_misses_are_absent_answers() {
    for (code, reason) in [
        ("hidden_command", "hidden_major"),
        ("not_found", "unknown_command"),
        ("not_found", "unknown_major"),
        ("not_found", "removed_major"),
    ] {
        assert!(verdict(code, reason, ApiStage::Resolve), "{code}/{reason}");
    }
    assert!(!verdict("not_found", "unknown_entity", ApiStage::Resolve));
    assert!(!verdict("not_found", "unknown_command", ApiStage::Session));
}
