//! Acceptance and invariant tests for `AGENT.PROPOSE.BRIEF` (`spec/agent/propose/brief/NODE.md`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_agent::{
    attach_brief, attach_brief_command_def, brief_schema, command_defs, invoke_attach_brief_cli,
    invoke_attach_brief_mcp, replay_brief_entries, BranchHeads, BriefEntry, BriefError, BriefJournal, Confidence,
    ProposalBrief, RootHandle, ATTACH_BRIEF,
};
use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse_with_limits, CValue};
use fid_journal::branch::BranchName;
use fid_mcp::taint::{taint_value, Untrusted};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/agent/propose")
}

fn load_branch_basic() -> (BranchHeads, Digest, Digest) {
    let raw = std::fs::read(fixtures_dir().join("branch-basic/project.json")).unwrap();
    let v = parse_with_limits(&raw, 64 * 1024, 16).unwrap();
    let CValue::Obj(root) = v else { panic!("project object") };
    let CValue::Obj(branches) = root.get("branches").unwrap() else { panic!("branches") };

    let mut heads = BranchHeads::new();
    let mut main_head = None;
    let mut agent_head = None;
    for (name, info) in branches {
        let CValue::Obj(bo) = info else { panic!("branch info") };
        let CValue::Str(h) = bo.get("head").unwrap() else { panic!("head") };
        let d = Digest::parse(h).unwrap();
        let bn = BranchName::parse(name).unwrap();
        if name == "main" {
            main_head = Some(d);
        } else if name == "agent/s1" {
            agent_head = Some(d);
        }
        heads.insert(bn, d);
    }
    (heads, main_head.unwrap(), agent_head.unwrap())
}

fn approved_roots() -> BTreeMap<String, PathBuf> {
    let mut roots = BTreeMap::new();
    // Map `root:` to repo `fixtures/agent/propose` via a parent so `root:fixtures/brief-valid.json`
    // or `root:brief-valid.json` can both be tested cleanly.
    roots.insert("root".into(), fixtures_dir());
    roots
}

/// AC-1: Attach valid brief on `agent/s1` via approved root handle CLI and via MCP; result holds
/// entry_id and bound_head equal to the head digest before the call, and the entry is on `agent/s1`
/// only (`main` is untouched).
#[test]
fn ac1_attach_valid_brief_on_agent_branch_cli_and_mcp_identical() {
    let (mut branches_cli, main_head, agent_head_before) = load_branch_basic();
    let (mut branches_mcp, _, _) = load_branch_basic();
    let mut journal_cli = BriefJournal::new();
    let mut journal_mcp = BriefJournal::new();
    let roots = approved_roots();
    let entry_id = Uuid::parse("00000000-0000-7000-8000-00000000a001").unwrap();

    let (cli_bytes, cli_entry) = invoke_attach_brief_cli(
        &roots,
        &mut branches_cli,
        &mut journal_cli,
        &["--branch", "agent/s1", "--brief", "root:brief-valid.json", "--json"],
        entry_id,
    )
    .unwrap();

    let brief_raw = std::fs::read_to_string(fixtures_dir().join("brief-valid.json")).unwrap();
    let mcp_params = format!(r#"{{"branch":"agent/s1","brief":{brief_raw}}}"#);
    let (mcp_bytes, mcp_entry) =
        invoke_attach_brief_mcp(&mut branches_mcp, &mut journal_mcp, mcp_params.as_bytes(), entry_id).unwrap();

    assert_eq!(cli_bytes, mcp_bytes);
    assert_eq!(cli_entry, mcp_entry);
    assert_eq!(cli_entry.entry_id, entry_id);
    assert_eq!(cli_entry.bound_head, agent_head_before);
    assert_eq!(cli_entry.branch.as_str(), "agent/s1");
    assert_eq!(cli_entry.brief.confidence, Confidence::Medium);

    // Entry is on agent/s1 only; main head is unchanged and has zero briefs.
    let main_bn = BranchName::parse("main").unwrap();
    let agent_bn = BranchName::parse("agent/s1").unwrap();
    assert_eq!(branches_cli.get(&main_bn), Some(main_head));
    assert_eq!(branches_cli.get(&agent_bn), Some(cli_entry.entry_digest));
    assert!(journal_cli.entries_on(&main_bn).is_empty());
    assert_eq!(journal_cli.entries_on(&agent_bn).len(), 1);

    // Result JSON carries entry_id and bound_head.
    let res_val = parse_with_limits(&cli_bytes, 4096, 4).unwrap();
    let CValue::Obj(ro) = res_val else { panic!("result obj") };
    assert_eq!(ro.get("entry_id"), Some(&CValue::Str(entry_id.to_string())));
    assert_eq!(ro.get("bound_head"), Some(&CValue::Str(agent_head_before.as_str().into())));
}

/// AC-2 (error path): Attaching to `main` (not an agent branch) fails with `not_agent_branch`
/// and leaves the journal and branch heads completely unchanged.
#[test]
fn ac2_non_agent_branch_fails_not_agent_branch_and_commits_nothing() {
    let (mut branches, _, _) = load_branch_basic();
    let branches_before = branches.clone();
    let mut journal = BriefJournal::new();
    let roots = approved_roots();
    let entry_id = Uuid::parse("00000000-0000-7000-8000-00000000a002").unwrap();

    let err = invoke_attach_brief_cli(
        &roots,
        &mut branches,
        &mut journal,
        &["--branch", "main", "--brief", "root:brief-valid.json", "--json"],
        entry_id,
    )
    .expect_err("main is not an agent branch");

    assert_eq!(err.code(), "not_agent_branch");
    assert_eq!(err, BriefError::NotAgentBranch { branch: "main".into() });
    assert_eq!(branches, branches_before);
    assert!(journal.entries().is_empty());
}

/// AC-3 (error path): `brief-bad.json` (2,001-char goal, 21 rationale steps, `confidence: 0.8`)
/// fails with `invalid_brief` naming ALL offending fields (`goal`, `rationale`, `confidence`),
/// and journals nothing. Also verifies unknown fields are rejected with `invalid_brief`.
#[test]
fn ac3_invalid_brief_reports_all_offending_fields_and_journals_nothing() {
    let (mut branches, _, _) = load_branch_basic();
    let branches_before = branches.clone();
    let mut journal = BriefJournal::new();
    let roots = approved_roots();
    let entry_id = Uuid::parse("00000000-0000-7000-8000-00000000a003").unwrap();

    let err = invoke_attach_brief_cli(
        &roots,
        &mut branches,
        &mut journal,
        &["--branch", "agent/s1", "--brief", "root:brief-bad.json", "--json"],
        entry_id,
    )
    .expect_err("brief-bad must fail");

    assert_eq!(err.code(), "invalid_brief");
    let fields = err.offending_fields();
    assert!(fields.contains(&"goal"), "expected goal in {fields:?}");
    assert!(fields.contains(&"rationale"), "expected rationale in {fields:?}");
    assert!(fields.contains(&"confidence"), "expected confidence in {fields:?}");
    assert_eq!(branches, branches_before);
    assert!(journal.entries().is_empty());

    // REQ-006: unknown field is rejected as invalid_brief.
    let valid_raw = std::fs::read(fixtures_dir().join("brief-valid.json")).unwrap();
    let CValue::Obj(mut map) = parse_with_limits(&valid_raw, 64 * 1024, 8).unwrap() else { panic!("obj") };
    map.insert("confidence_score".into(), CValue::Num(0.95));
    let extra_err = attach_brief(&mut branches, &mut journal, "agent/s1", &CValue::Obj(map), entry_id)
        .expect_err("unknown field must fail");
    assert_eq!(extra_err.code(), "invalid_brief");
    assert!(extra_err.offending_fields().contains(&"confidence_score"));
    assert!(journal.entries().is_empty());
}

/// AC-4 + REQ-005: Reading the brief back through `AUTO.MCP.TAINT` (`fid_mcp::taint::taint_value`)
/// against `brief_schema()` wraps EVERY free-text field (`goal`, `rationale[*]`, `assumptions[*]`,
/// `confidence_reasons[*]`, `client`, `model`) as `{"untrusted": true, "text": ...}` while keeping
/// trusted enum `confidence` unwrapped.
#[test]
fn ac4_mcp_taint_wraps_every_free_text_field_as_untrusted() {
    let raw = std::fs::read(fixtures_dir().join("brief-valid.json")).unwrap();
    let brief = ProposalBrief::from_json(&raw).unwrap();
    let tainted = taint_value(&brief.to_value(), &brief_schema());
    let CValue::Obj(to) = tainted else { panic!("tainted brief must be object") };

    // Single untrusted string fields
    for key in ["goal", "client", "model"] {
        let got = Untrusted::from_value(to.get(key).unwrap())
            .unwrap_or_else(|| panic!("expected untrusted wrapper on {key}, got {:?}", to.get(key)));
        assert!(!got.is_empty(), "{key} text should be preserved");
    }

    // Array-of-untrusted-string fields
    for key in ["rationale", "assumptions", "confidence_reasons"] {
        let CValue::Arr(items) = to.get(key).unwrap() else { panic!("{key} must be array") };
        assert!(!items.is_empty());
        for (i, item) in items.iter().enumerate() {
            assert!(Untrusted::from_value(item).is_some(), "expected untrusted wrapper on {key}[{i}], got {item:?}");
        }
    }

    // Closed enum `confidence` remains a trusted plain string.
    assert_eq!(to.get("confidence"), Some(&CValue::Str("medium".into())));
}

/// AC-5 + REQ-007: Attaching a second brief on `agent/s1` appends a second entry bound to the
/// first entry's digest, and `current_brief` reports the second entry.
#[test]
fn ac5_latest_attach_wins_as_current_brief() {
    let (mut branches, _, first_head) = load_branch_basic();
    let mut journal = BriefJournal::new();
    let roots = approved_roots();
    let id1 = Uuid::parse("00000000-0000-7000-8000-00000000b001").unwrap();
    let id2 = Uuid::parse("00000000-0000-7000-8000-00000000b002").unwrap();

    let (_, e1) = invoke_attach_brief_cli(
        &roots,
        &mut branches,
        &mut journal,
        &["--branch", "agent/s1", "--brief", "root:brief-valid.json", "--json"],
        id1,
    )
    .unwrap();
    let (_, e2) = invoke_attach_brief_cli(
        &roots,
        &mut branches,
        &mut journal,
        &["--branch", "agent/s1", "--brief", "root:brief-valid-2.json", "--json"],
        id2,
    )
    .unwrap();

    assert_eq!(e1.bound_head, first_head);
    assert_eq!(e2.bound_head, e1.entry_digest);
    let agent_bn = BranchName::parse("agent/s1").unwrap();
    assert_eq!(journal.entries_on(&agent_bn).len(), 2);
    assert_eq!(journal.current_brief(&agent_bn), Some(&e2));
    assert_eq!(journal.current_brief(&agent_bn).unwrap().brief.confidence, Confidence::High);
}

/// AC-6 (replay): Serializing the journal entries to canonical JSON, decoding them back, and
/// replaying from the initial branch heads produces `IDENTICAL` branch heads and journal state.
#[test]
fn ac6_exact_replay_is_identical() {
    let (initial_branches, _, _) = load_branch_basic();
    let mut branches = initial_branches.clone();
    let mut journal = BriefJournal::new();
    let roots = approved_roots();
    let id1 = Uuid::parse("00000000-0000-7000-8000-00000000c001").unwrap();
    let id2 = Uuid::parse("00000000-0000-7000-8000-00000000c002").unwrap();

    let _ = invoke_attach_brief_cli(
        &roots,
        &mut branches,
        &mut journal,
        &["--branch", "agent/s1", "--brief", "root:brief-valid.json"],
        id1,
    )
    .unwrap();
    let _ = invoke_attach_brief_cli(
        &roots,
        &mut branches,
        &mut journal,
        &["--branch", "agent/s1", "--brief", "root:brief-valid-2.json"],
        id2,
    )
    .unwrap();

    // Round-trip every entry through canonical JSON.
    let decoded: Vec<BriefEntry> =
        journal.entries().iter().map(|e| BriefEntry::from_json(&e.to_json().unwrap()).unwrap()).collect();

    let (replayed_branches, replayed_journal) = replay_brief_entries(&initial_branches, &decoded).unwrap();
    assert_eq!(replayed_branches, branches);
    assert_eq!(replayed_branches.state_digest().unwrap(), branches.state_digest().unwrap());
    assert_eq!(replayed_journal, journal);
}

/// C5/C6: CLI rejects raw paths, traversal segments and unknown root aliases; Unicode scalar
/// counts accept 2,000 multi-byte scalars and reject 2,001.
#[test]
fn root_handle_rejects_raw_paths_and_unicode_char_counts_hold() {
    for bad in [
        "/etc/passwd",
        "brief-valid.json",
        "root:../secret.json",
        "root:/abs.json",
        "root:a\\b.json",
        ":brief.json",
        "root:",
    ] {
        assert!(RootHandle::parse(bad).is_err(), "raw/bad handle should fail: {bad}");
    }
    assert!(RootHandle::parse("root:fixtures/brief-valid.json").is_ok());

    // 2,000 multi-byte Unicode scalars (`µ`, 2 UTF-8 bytes each = 4,000 bytes) is valid!
    let valid_raw = std::fs::read(fixtures_dir().join("brief-valid.json")).unwrap();
    let CValue::Obj(mut map) = parse_with_limits(&valid_raw, 64 * 1024, 8).unwrap() else { panic!("obj") };
    map.insert("goal".into(), CValue::Str("µ".repeat(2_000)));
    assert!(ProposalBrief::from_value(&CValue::Obj(map.clone())).is_ok());
    map.insert("goal".into(), CValue::Str("µ".repeat(2_001)));
    let err = ProposalBrief::from_value(&CValue::Obj(map)).expect_err("2001 scalars must fail");
    assert_eq!(err.offending_fields(), vec!["goal"]);
}

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities")))
    }

    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

/// Registry definition check: `agent.attach_brief@1` is W1, D0, `fs_effect: none`, and its
/// params/result schemas compile and validate through `fid-bus::validate::Compiled`.
#[test]
fn command_def_is_w1_d0_and_schemas_validate_with_bus() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 2);
    assert_eq!(format!("{}@{}", defs[0].id, defs[0].major), ATTACH_BRIEF);
    let def = attach_brief_command_def().unwrap();
    assert_eq!(format!("{}@{}", def.id, def.major), ATTACH_BRIEF);
    assert_eq!(def.side_effect.as_str(), "W1");
    assert_eq!(def.determinism.as_str(), "D0");
    assert_eq!(def.fs_effect.as_str(), "none");
    assert_eq!(def.undo.as_str(), "head_move");

    let params_schema = Compiled::compile(&def.params_schema).expect("params schema compiles");
    let result_schema = Compiled::compile(&def.result_schema).expect("result schema compiles");

    let raw = std::fs::read(fixtures_dir().join("brief-valid.json")).unwrap();
    let brief = ProposalBrief::from_json(&raw).unwrap();
    let mut params = BTreeMap::new();
    params.insert("branch".into(), CValue::Str("agent/s1".into()));
    params.insert("brief".into(), brief.to_value());
    assert!(params_schema.evaluate(&CValue::Obj(params), &NoHooks).is_ok());

    let mut res = BTreeMap::new();
    res.insert("bound_head".into(), CValue::Str(Digest::from_bytes([0xab; 32]).as_str().into()));
    res.insert("entry_id".into(), CValue::Str("00000000-0000-7000-8000-00000000a001".into()));
    assert!(result_schema.evaluate(&CValue::Obj(res), &NoHooks).is_ok());
}
