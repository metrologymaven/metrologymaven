//! Acceptance and unit tests for `FLOW.RUNNER.UNASSIST` (`fid_accept::unassist`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Instant;

use fid_accept::unassist::{check_unassisted, JournalReader, Stream, UnassistError, ViolationCode};

fn fix(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures/flow/unassist")
        .join(name)
}

/// AC-1: 20 human and script entries -> zero violations.
#[test]
fn ac1_human_only_has_no_violations() {
    let reader = JournalReader::from_path(fix("human-only.jrn"));
    let violations = check_unassisted(&reader).unwrap();
    assert!(violations.is_empty(), "expected 0 violations, got: {violations:?}");
}

/// AC-2: entry 7 attributed to an agent -> exactly one violation at index 7.
#[test]
fn ac2_one_agent_reports_single_violation_at_index_7() {
    let reader = JournalReader::from_path(fix("one-agent.jrn"));
    let violations = check_unassisted(&reader).unwrap();
    assert_eq!(violations.len(), 1, "got: {violations:?}");
    let v = &violations[0];
    assert_eq!(v.index, 7);
    assert_eq!(v.stream, Stream::Record);
    assert_eq!(v.command_id, "fit.ls_circle@1");
    assert!(v.principal.starts_with("agent:"), "principal={}", v.principal);
    assert_eq!(v.code, ViolationCode::AgentPrincipal);
}

/// AC-3: missing principal at index 4 -> UNATTRIBUTED at index 4.
#[test]
fn ac3_unattributed_reports_unattributed_at_affected_index() {
    let reader = JournalReader::from_path(fix("unattributed.jrn"));
    let violations = check_unassisted(&reader).unwrap();
    assert_eq!(violations.len(), 1, "got: {violations:?}");
    let v = &violations[0];
    assert_eq!(v.index, 4);
    assert_eq!(v.stream, Stream::Record);
    assert_eq!(v.command_id, "report.export_pdf@1");
    assert_eq!(v.principal, "<none>");
    assert_eq!(v.code, ViolationCode::Unattributed);
}

/// REQ-001: MCP-opened session flags every entry in that session, including after a human upgrade.
#[test]
fn mcp_channel_session_and_upgrade_flagged() {
    let reader = JournalReader::from_path(fix("mcp-session.jrn"));
    let violations = check_unassisted(&reader).unwrap();
    assert_eq!(violations.len(), 4, "got: {violations:?}");
    for (idx, v) in violations.iter().enumerate() {
        assert_eq!(v.index, idx);
        assert_eq!(v.stream, Stream::Record);
        assert_eq!(v.code, ViolationCode::McpChannel);
    }
    assert_eq!(violations[0].command_id, "session_header");
    assert_eq!(violations[1].command_id, "fit.ls_plane@1");
    assert_eq!(violations[2].command_id, "principal_upgrade");
    assert_eq!(violations[3].command_id, "report.export_pdf@1");
}

/// REQ-001: human command targeting an `agent/` branch is flagged as `AGENT_BRANCH`.
#[test]
fn agent_branch_flagged_even_for_human_principal() {
    let reader = JournalReader::from_path(fix("agent-branch.jrn"));
    let violations = check_unassisted(&reader).unwrap();
    assert_eq!(violations.len(), 1, "got: {violations:?}");
    assert_eq!(violations[0].index, 2);
    assert_eq!(violations[0].command_id, "fit.ls_circle@1");
    assert_eq!(violations[0].code, ViolationCode::AgentBranch);
}

/// REQ-002: Activity Log entry by an agent is reported with `Stream::Activity`.
#[test]
fn activity_log_agent_entry_flagged() {
    let reader = JournalReader::from_paths(fix("human-only.jrn"), Some(fix("activity-agent.jsonl")));
    let violations = check_unassisted(&reader).unwrap();
    assert_eq!(violations.len(), 1, "got: {violations:?}");
    let v = &violations[0];
    assert_eq!(v.index, 1);
    assert_eq!(v.stream, Stream::Activity);
    assert_eq!(v.command_id, "project.describe_element@1");
    assert_eq!(v.code, ViolationCode::AgentPrincipal);
}

/// Compact JSONL golden format (`fixtures/flow/pkg/valid/journal.jsonl`) is also accepted cleanly.
#[test]
fn compact_golden_journal_format_supported() {
    let compact = concat!(
        "{\"seq\":\"1\",\"command\":\"import.cad_step@1\",\"principal\":\"human\"}\n",
        "{\"seq\":\"2\",\"command\":\"align.run_bestfit@1\",\"principal\":\"agent\"}\n",
        "{\"seq\":\"3\",\"command\":\"report.export_pdf@1\"}\n",
    );
    let reader = JournalReader::from_bytes(compact.as_bytes().to_vec());
    let violations = check_unassisted(&reader).unwrap();
    assert_eq!(violations.len(), 2);
    assert_eq!((violations[0].index, violations[0].code), (1, ViolationCode::AgentPrincipal));
    assert_eq!((violations[1].index, violations[1].code), (2, ViolationCode::Unattributed));
}

/// Read / parse errors propagate as `UNASSIST-READ`.
#[test]
fn malformed_and_missing_inputs_return_unassist_read() {
    let missing = JournalReader::from_path(fix("does-not-exist.jrn"));
    let err = check_unassisted(&missing).unwrap_err();
    assert_eq!(err.code(), "UNASSIST-READ");
    assert!(matches!(err, UnassistError::Read(_)));

    let bad_json = JournalReader::from_bytes(b"{not valid json}\n".to_vec());
    assert_eq!(check_unassisted(&bad_json).unwrap_err().code(), "UNASSIST-READ");

    let blank_line = JournalReader::from_bytes(b"{\"principal\":\"human\"}\n\n".to_vec());
    assert_eq!(check_unassisted(&blank_line).unwrap_err().code(), "UNASSIST-READ");
}

/// Performance budget: 100,000 entries stream cleanly with zero violations.
#[test]
fn stream_100k_entries_succeeds() {
    let line = "{\"seq\":\"1\",\"command\":\"fit.ls_plane@1\",\"principal\":\"human\"}\n";
    let mut data = Vec::with_capacity(line.len() * 100_000);
    for _ in 0..100_000 {
        data.extend_from_slice(line.as_bytes());
    }
    let reader = JournalReader::from_bytes(data);
    let t0 = Instant::now();
    let violations = check_unassisted(&reader).unwrap();
    let elapsed = t0.elapsed();
    assert!(violations.is_empty());
    // Generous debug-mode ceiling so CI never flakes; release mode is well under 1 s.
    assert!(elapsed.as_secs() < 30, "100k entries took {elapsed:?}");
}
