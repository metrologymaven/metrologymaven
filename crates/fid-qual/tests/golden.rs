//! Integration tests for spec node `QUAL.IQOQ.GOLDEN` (AC-1..AC-4, REQ-001..004,
//! `fid-bus` `human_only: true` policy & JSON Schema checks, `CORE.JRN.ENTRY`
//! commitment verification, `active_goldens()` for OQ, and 10,000-entry perf budget).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, PolicyReason, ValidationReason};
use fid_bus::policy::class::{class_rule, ClassRule};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{digest_bytes, parse, CValue, Digest as CanonDigest};
use fid_journal::entry::{
    verify_commitment, EntryTime, Field, PrincipalKind, RecordedSalts, Salt, FIRST_SEQ, GENESIS_PREV,
};
use fid_qual::all_command_defs;
use fid_qual::golden::{
    build_register_golden_entry, command_def, invoke_cli, invoke_mcp, register_golden, Golden, GoldenError,
    GoldenJournal, ProjectEntry, RegisterGoldenCall, Untrusted, ERR_GOLDEN_EMPTY, ERR_GOLDEN_UNKNOWN,
    ERR_POLICY_REFUSED, MCP_REGISTER_GOLDEN, REGISTER_GOLDEN_ID,
};
use fid_registry::{Catalogue, CommandDecl};
use fid_surface::{Envelope, ErrorCode, Outcome};
use fid_types::{Determinism, Digest, SideEffect};
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

const TEST_UNIX_MS: u64 = 1_795_363_200_000;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/qual/iqoq/golden")
}

fn load_plate() -> GoldenJournal {
    GoldenJournal::load_file(&fixture_dir().join("plate.fid")).expect("load plate.fid")
}

fn load_expected() -> BTreeMap<String, CValue> {
    let bytes = std::fs::read(fixture_dir().join("expected.json")).expect("read expected.json");
    match parse(&bytes).expect("parse expected.json") {
        CValue::Obj(o) => o,
        other => panic!("expected object in expected.json, got {other:?}"),
    }
}

fn exp_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match m.get(k) {
        Some(CValue::Str(s)) => s.as_str(),
        other => panic!("expected string for {k}, got {other:?}"),
    }
}

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn sample_salts() -> RecordedSalts {
    let mut m = BTreeMap::new();
    for (idx, field) in
        [Field::Args, Field::Events, Field::Identity, Field::OnBehalfOf, Field::Reason, Field::UpgradeIdentity]
            .into_iter()
            .enumerate()
    {
        m.insert(field, Salt([(idx as u8) + 1; 16]));
    }
    RecordedSalts(m)
}

#[test]
fn ac1_human_registers_plate_golden_with_pinned_digest() {
    let mut journal = load_plate();
    let expected = load_expected();
    let pinned_digest = Digest::parse(exp_str(&expected, "expected_digest")).expect("pinned digest");
    let pinned_kernel = Digest::parse(exp_str(&expected, "kernel_manifest_digest")).expect("pinned kernel");
    let pinned_version = exp_str(&expected, "product_version");

    let out = invoke_cli(
        &mut journal,
        PrincipalKind::Human,
        TEST_UNIX_MS,
        &["register-golden", "--branch", "main", "--from", "10", "--to", "42", "--label", "plate-v1", "--json"],
    );
    assert_eq!(out.stderr, "");
    assert_eq!(out.envelope.outcome, Outcome::Committed);
    assert_eq!(out.envelope.command_id, REGISTER_GOLDEN_ID);
    assert!(out.envelope.journal_entry_hash.is_some());

    let decoded = Envelope::decode(&out.stdout).expect("decode canonical CLI envelope");
    assert_eq!(decoded, out.envelope);
    let result = decoded.result.expect("golden result");
    let golden = Golden::from_value(&result).expect("parse Golden");

    assert_eq!(golden.id.version(), 7, "ID must be RFC 9562 UUIDv7");
    assert_eq!(golden.id.variant_bits(), 0b10);
    assert_eq!(golden.label.as_str(), "plate-v1");
    assert_eq!(golden.branch, "main");
    assert_eq!(golden.from_seq, 10);
    assert_eq!(golden.to_seq, 42);
    assert_eq!(golden.product_version, pinned_version);
    assert_eq!(golden.kernel_manifest_digest, pinned_kernel);
    assert_eq!(golden.expected_digest, pinned_digest);
    assert!(!golden.retired);
    assert_eq!(golden.supersedes, None);

    // Stored in journal and exposed by active_goldens() for OQ
    assert_eq!(journal.committed_events().len(), 1);
    let active = journal.active_goldens();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0], &golden);

    // Verifies cleanly as a CORE.JRN.ENTRY journal commitment
    let mut salts = sample_salts();
    let entry = build_register_golden_entry(
        &golden,
        FIRST_SEQ,
        GENESIS_PREV,
        EntryTime { utc: "2026-11-22T14:00:00Z".to_string(), mono_ns: 1_000_000, sync: "synced".to_string() },
        "qa-engineer@example.invalid",
        CanonDigest(digest_bytes(b"registry-manifest").0),
        &mut salts,
    )
    .expect("build journal entry");
    entry.header.verify_hash().expect("journal header hash verifies");
    verify_commitment(
        &entry.header,
        Field::Events,
        entry.payload.salts.get(&Field::Events),
        entry.payload.values.get(&Field::Events),
    )
    .expect("journal events commitment verifies");
}

#[test]
fn ac2_non_human_principals_refused_with_policy_refused_and_nothing_journaled() {
    let params = br#"{"branch":"main","from_seq":10,"to_seq":42,"label":"plate-v1"}"#;

    for principal in [
        PrincipalKind::Agent,
        PrincipalKind::Script,
        PrincipalKind::Service,
        PrincipalKind::Device,
        PrincipalKind::System,
    ] {
        let mut journal = load_plate();
        let env = invoke_mcp(&mut journal, principal, TEST_UNIX_MS, MCP_REGISTER_GOLDEN, params);
        assert_eq!(env.outcome, Outcome::Failed);
        assert_eq!(env.command_id, REGISTER_GOLDEN_ID);
        assert_eq!(env.journal_entry_hash, None);
        let err = env.error.expect("policy_refused error");
        assert_eq!(err.code, ErrorCode::PolicyRefused);
        assert_eq!(err.code.as_str(), ERR_POLICY_REFUSED);
        assert!(journal.committed_events().is_empty(), "nothing committed for {principal:?}");
        assert!(journal.active_goldens().is_empty());

        // Direct bus policy check also refuses via `human_only: true`
        let def = command_def().expect("command_def");
        assert_eq!(
            class_rule(principal, def.side_effect, def.human_only),
            ClassRule::Refuse(PolicyReason::HumanOnly),
            "bus class_rule must refuse {principal:?}"
        );
    }

    // Human is permitted by bus policy check
    let def = command_def().expect("command_def");
    assert_eq!(class_rule(PrincipalKind::Human, def.side_effect, def.human_only), ClassRule::Allow);
}

#[test]
fn ac3_empty_range_fails_with_validate_golden_empty_and_commits_nothing() {
    let mut journal = load_plate();

    let out = invoke_cli(
        &mut journal,
        PrincipalKind::Human,
        TEST_UNIX_MS,
        &["register-golden", "--branch", "main", "--from", "1", "--to", "9", "--label", "no-results", "--json"],
    );
    assert_eq!(out.envelope.outcome, Outcome::Failed);
    let err = out.envelope.error.expect("golden_empty error");
    assert_eq!(err.code, ErrorCode::ValidationError);
    let details = match err.details {
        Some(CValue::Obj(m)) => m,
        other => panic!("expected details object, got {other:?}"),
    };
    assert_eq!(details.get("qual_error"), Some(&CValue::Str(ERR_GOLDEN_EMPTY.to_string())));
    assert!(journal.committed_events().is_empty());
    assert!(journal.active_goldens().is_empty());

    // Direct call on a branch with no entries also fails with GoldenEmpty
    let call = RegisterGoldenCall {
        branch: "other-branch".to_string(),
        from_seq: 10,
        to_seq: 42,
        label: Untrusted::label("other").expect("label"),
        supersedes: None,
    };
    let e = register_golden(&mut journal, PrincipalKind::Human, &call, TEST_UNIX_MS).unwrap_err();
    assert_eq!(e.code(), ERR_GOLDEN_EMPTY);
}

#[test]
fn ac4_supersedes_retires_earlier_golden_and_excludes_it_from_active_goldens() {
    let mut journal = load_plate();

    let out1 = invoke_cli(
        &mut journal,
        PrincipalKind::Human,
        TEST_UNIX_MS,
        &["register-golden", "--branch", "main", "--from", "10", "--to", "42", "--label", "plate-v1", "--json"],
    );
    let g1 = Golden::from_value(&out1.envelope.result.expect("g1")).expect("parse g1");
    assert_eq!(journal.active_goldens().len(), 1);

    let g1_id = g1.id.to_string();
    let out2 = invoke_cli(
        &mut journal,
        PrincipalKind::Human,
        TEST_UNIX_MS + 1_000,
        &[
            "register-golden",
            "--branch",
            "main",
            "--from",
            "10",
            "--to",
            "42",
            "--label",
            "plate-v2",
            "--supersedes",
            &g1_id,
            "--json",
        ],
    );
    assert_eq!(out2.envelope.outcome, Outcome::Committed);
    let g2 = Golden::from_value(&out2.envelope.result.expect("g2")).expect("parse g2");
    assert_eq!(g2.supersedes, Some(g1.id));
    assert!(!g2.retired);
    assert_ne!(g1.id, g2.id);

    // First golden is marked retired and excluded from active_goldens()
    let stored_g1 = journal.get_golden(&g1.id).expect("g1 still stored in history");
    assert!(stored_g1.retired, "first golden must be retired");
    let active = journal.active_goldens();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, g2.id);
    assert_eq!(journal.all_goldens().len(), 2);

    // Unknown supersedes ID fails with validate.golden_unknown and leaves state unchanged
    let unknown_id = Uuid::parse("019a0000-0000-7000-8000-000000000099").expect("v7");
    let bad_call = RegisterGoldenCall {
        branch: "main".to_string(),
        from_seq: 10,
        to_seq: 42,
        label: Untrusted::label("plate-v3").expect("label"),
        supersedes: Some(unknown_id),
    };
    let err = register_golden(&mut journal, PrincipalKind::Human, &bad_call, TEST_UNIX_MS + 2_000).unwrap_err();
    assert_eq!(err, GoldenError::GoldenUnknown { id: unknown_id });
    assert_eq!(err.code(), ERR_GOLDEN_UNKNOWN);
    assert_eq!(journal.active_goldens().len(), 1);
}

#[test]
fn registry_command_def_bus_schemas_and_all_command_defs() {
    let def = command_def().expect("command_def");
    assert_eq!(def.id, "validate.register_golden");
    assert_eq!(def.major, 1);
    assert_eq!(def.side_effect, SideEffect::W2);
    assert_eq!(def.determinism, Determinism::D0);
    assert!(def.human_only);
    assert_eq!(def.mcp_projection.as_str(), "request_only");

    // Exports cleanly with validate.show_record@1 through CORE.REG.VAL
    let all = all_command_defs().expect("all_command_defs");
    assert_eq!(all.len(), 2);
    let cat = Catalogue::geo_frame().expect("geo_frame catalogue");
    let decls: Vec<CommandDecl> = all.iter().map(CommandDecl::from_def).collect();
    let export = fid_registry::manifest::export_with(&decls, &cat).expect("export all fid-qual commands");
    assert_eq!(export.definitions.len(), 2);

    // Bus JSON Schema validates real params and results
    let params_compiled = Compiled::compile(&def.params_schema).expect("compile params_schema");
    let result_compiled = Compiled::compile(&def.result_schema).expect("compile result_schema");

    let call = RegisterGoldenCall {
        branch: "main".to_string(),
        from_seq: 10,
        to_seq: 42,
        label: Untrusted::label("plate-v1").expect("label"),
        supersedes: None,
    };
    assert!(params_compiled.evaluate(&call.params(), &NoHooks).is_ok());

    let mut journal = load_plate();
    let (golden, _) = register_golden(&mut journal, PrincipalKind::Human, &call, TEST_UNIX_MS).expect("register");
    assert!(result_compiled.evaluate(&golden.to_value(), &NoHooks).is_ok());

    // Rejects unknown parameter and bad label
    let mut bad_params = BTreeMap::new();
    bad_params.insert("branch".to_string(), CValue::Str("main".to_string()));
    bad_params.insert("from_seq".to_string(), CValue::Num(10.0));
    bad_params.insert("to_seq".to_string(), CValue::Num(42.0));
    bad_params.insert("label".to_string(), CValue::Str("ok".to_string()));
    bad_params.insert("extra".to_string(), CValue::Bool(true));
    assert!(params_compiled.evaluate(&CValue::Obj(bad_params), &NoHooks).is_err());

    // Control characters rejected in untrusted label
    assert!(Untrusted::label("bad\nlabel").is_err());
    assert!(Untrusted::label("").is_err());
}

#[test]
fn performance_budget_under_1s_for_10000_entries() {
    let mut journal = GoldenJournal::new();
    let kernel = Digest::from_bytes(digest_bytes(b"perf-kernel").0);
    for seq in 1..=10_000u64 {
        let rd =
            if seq % 2 == 0 { Some(Digest::from_bytes(digest_bytes(format!("res-{seq}").as_bytes()).0)) } else { None };
        journal
            .push_entry(ProjectEntry {
                branch: "main".to_string(),
                seq,
                command_id: "fit.circle@1".to_string(),
                product_version: "1.4.0".to_string(),
                kernel_manifest_digest: kernel,
                result_record_digest: rd,
            })
            .expect("push");
    }

    let call = RegisterGoldenCall {
        branch: "main".to_string(),
        from_seq: 1,
        to_seq: 10_000,
        label: Untrusted::label("ten-k").expect("label"),
        supersedes: None,
    };
    let start = Instant::now();
    let (golden, _) = register_golden(&mut journal, PrincipalKind::Human, &call, TEST_UNIX_MS).expect("register 10k");
    let elapsed = start.elapsed();
    assert!(elapsed.as_millis() < 1_000, "took {elapsed:?}, budget < 1s");
    assert_eq!(golden.from_seq, 1);
    assert_eq!(golden.to_seq, 10_000);
}
