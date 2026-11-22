//! Integration tests for spec node `QUAL.IQOQ.RECORD` (AC-1..AC-5, REQ-001..005,
//! CORE.JRN.ENTRY bridge, registry/bus schema compilation, fail-closed validation, and
//! 10,000-event performance budget).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue, Digest as CanonDigest};
use fid_journal::entry::{
    AiAssistance, EntryTime, Field, Manifests, PrincipalDraft, PrincipalKind, RecordedSalts, Salt, FIRST_SEQ,
    GENESIS_PREV,
};
use fid_journal::stream::{route, Locality, RouteContext, StreamId};
use fid_qual::{
    build_journal_entry, command_defs, extract_from_entry, invoke_cli, invoke_mcp, ActivityEntry, CheckResult,
    GoldenExceedance, GoldenOutcome, GoldenResult, JournalMeta, QualError, QualificationRecord, QualificationStatus,
    RecordJournal, RecordKind, StatusCode, Surface, Verdict, ERR_RECORD_NOT_FOUND, MCP_SHOW_RECORD, RECORD_SCHEMA,
    RUN_IQ_ID, RUN_OQ_ID, SHOW_RECORD_ID,
};
use fid_registry::{Catalogue, CommandDecl};
use fid_surface::{Envelope, ErrorCode, Outcome};
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/qual/iqoq/records")
}

fn load_v14_fixture() -> RecordJournal {
    RecordJournal::load_file(&fixture_dir().join("v1.4-qualified.fid")).expect("load v1.4-qualified.fid")
}

fn load_expected() -> BTreeMap<String, CValue> {
    let bytes = std::fs::read(fixture_dir().join("expected.json")).expect("read expected.json");
    match parse(&bytes).expect("parse expected.json") {
        CValue::Obj(o) => o,
        other => panic!("expected object in expected.json, got {other:?}"),
    }
}

fn synthetic_digest(tag: &str) -> Digest {
    Digest::from_bytes(digest_bytes(tag.as_bytes()).0)
}

fn sample_iq(version: &str, verdict: Verdict) -> QualificationRecord {
    QualificationRecord {
        schema: RECORD_SCHEMA.to_string(),
        kind: RecordKind::Iq,
        product_version: version.to_string(),
        kernel_manifest_digest: synthetic_digest(&format!("{version}:kernel")),
        evidence_statement_digest: synthetic_digest(&format!("{version}:evidence")),
        env_digest: synthetic_digest(&format!("{version}:env")),
        checks: vec![
            CheckResult {
                id: "artefact:fid-cli".to_string(),
                verdict,
                detail: if verdict == Verdict::Pass { "MATCH" } else { "MISMATCH" }.to_string(),
            },
            CheckResult { id: "sig:sums".to_string(), verdict: Verdict::Pass, detail: "verified".to_string() },
        ],
        verdict,
        rdl_subset_digest: None,
        goldens: None,
    }
}

fn sample_oq(version: &str, verdict: Verdict) -> QualificationRecord {
    let goldens = if verdict == Verdict::Pass {
        vec![GoldenResult {
            golden_id: "golden-1".to_string(),
            outcome: GoldenOutcome::Identical,
            reasons: Vec::new(),
            exceedances: Vec::new(),
        }]
    } else {
        vec![GoldenResult {
            golden_id: "golden-1".to_string(),
            outcome: GoldenOutcome::Divergent,
            reasons: vec!["value_exceeded_rep1".to_string()],
            exceedances: vec![GoldenExceedance {
                characteristic: "cyl.diam.01".to_string(),
                delta: "0.0025".to_string(),
                decision_changed: true,
            }],
        }]
    };
    QualificationRecord {
        schema: RECORD_SCHEMA.to_string(),
        kind: RecordKind::Oq,
        product_version: version.to_string(),
        kernel_manifest_digest: synthetic_digest(&format!("{version}:kernel")),
        evidence_statement_digest: synthetic_digest(&format!("{version}:evidence")),
        env_digest: synthetic_digest(&format!("{version}:env")),
        checks: vec![CheckResult {
            id: "rdl:fit-circle-lsq-001".to_string(),
            verdict,
            detail: if verdict == Verdict::Pass { "within REP-1" } else { "exceeded REP-1" }.to_string(),
        }],
        verdict,
        rdl_subset_digest: Some(synthetic_digest(&format!("{version}:rdl-subset"))),
        goldens: Some(goldens),
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

fn sample_meta(version: &str) -> JournalMeta {
    JournalMeta {
        stream: "record".to_string(),
        branch: "main".to_string(),
        logical_parent: None,
        time: EntryTime { utc: "2026-11-22T07:00:00Z".to_string(), mono_ns: 1_000_000, sync: "synced".to_string() },
        principal: PrincipalDraft {
            kind: PrincipalKind::Human,
            identity: CValue::Str("qa-engineer@example.invalid".to_string()),
            on_behalf_of: None,
        },
        ai_assistance: AiAssistance::None,
        manifests: Manifests {
            registry: CanonDigest(synthetic_digest("registry-manifest").to_bytes()),
            kernel: CanonDigest(synthetic_digest(&format!("{version}:kernel")).to_bytes()),
        },
        args: CValue::Obj(BTreeMap::new()),
        reason: Some("release qualification".to_string()),
    }
}

#[test]
fn ac1_v14_fixture_is_qualified_with_both_record_ids() {
    let journal = load_v14_fixture();
    let expected = load_expected();
    let mut activity_log = Vec::new();

    let out = invoke_cli(&journal, "1.4.0", &mut activity_log, &["show-record", "--json"]);
    assert_eq!(out.stderr, "");
    assert_eq!(out.envelope.outcome, Outcome::Computed);
    assert_eq!(out.envelope.command_id, SHOW_RECORD_ID);
    assert_eq!(out.envelope.journal_entry_hash, None);

    let decoded = Envelope::decode(&out.stdout).expect("decode canonical CLI envelope");
    assert_eq!(decoded, out.envelope);
    let result = decoded.result.expect("status result");
    assert_eq!(&result, expected.get("ac1_status_1_4_0").expect("expected ac1"));

    let status = QualificationStatus::from_value(&result).expect("parse status");
    assert_eq!(status.status, StatusCode::Qualified);
    assert_eq!(status.product_version, "1.4.0");
    assert!(status.iq_record.is_some());
    assert!(status.oq_record.is_some());
    assert_eq!(activity_log.len(), 1);
    assert_eq!(activity_log[0].surface, Surface::Cli);
}

#[test]
fn ac2_v14_fixture_under_1_5_0_is_unqualified_new_version() {
    let journal = load_v14_fixture();
    let expected = load_expected();
    let mut activity_log = Vec::new();

    let out = invoke_cli(&journal, "1.5.0", &mut activity_log, &["show-record", "--json"]);
    assert_eq!(out.envelope.outcome, Outcome::Computed);
    let result = out.envelope.result.expect("status result");
    assert_eq!(&result, expected.get("ac2_status_1_5_0").expect("expected ac2"));

    let status = QualificationStatus::from_value(&result).expect("parse status");
    assert_eq!(status.status, StatusCode::UnqualifiedNewVersion);
    assert_eq!(status.product_version, "1.5.0");
    assert_eq!(status.iq_record, None);
    assert_eq!(status.oq_record, None);
}

#[test]
fn ac3_show_by_id_returns_exact_journaled_body_matching_result_digest() {
    let journal = load_v14_fixture();
    let expected = load_expected();
    let mut activity_log = Vec::new();

    let status_out = invoke_cli(&journal, "1.4.0", &mut activity_log, &["show-record", "--json"]);
    let status = QualificationStatus::from_value(status_out.envelope.result.as_ref().unwrap()).unwrap();
    let iq_id = status.iq_record.expect("iq id");
    let oq_id = status.oq_record.expect("oq id");

    for (id, expected_digest_key) in [(&iq_id, "iq_result_digest"), (&oq_id, "oq_result_digest")] {
        let out = invoke_cli(&journal, "1.4.0", &mut activity_log, &["show-record", "--id", id, "--json"]);
        assert_eq!(out.envelope.outcome, Outcome::Computed);
        let body = out.envelope.result.as_ref().expect("record result");
        let journal_ev = journal.get_record(id).expect("journal event");
        assert_eq!(body, &journal_ev.record.to_value());

        let canonical = to_canonical_bytes(body).expect("canonical bytes");
        let sha = Digest::from_bytes(digest_bytes(&canonical).0);
        assert_eq!(sha, journal_ev.result_digest);
        assert_eq!(CValue::Str(sha.as_str().to_string()), *expected.get(expected_digest_key).unwrap());

        let roundtrip = QualificationRecord::from_json(&canonical).expect("roundtrip record");
        assert_eq!(roundtrip, journal_ev.record);
        assert_eq!(roundtrip.result_digest().unwrap(), journal_ev.result_digest);
    }
}

#[test]
fn ac4_unknown_id_fails_with_record_not_found_and_writes_nothing_to_record_journal() {
    let journal = load_v14_fixture();
    let snapshot_before = journal.clone();
    let mut activity_log = Vec::new();
    let missing_id = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    let err = journal.get_record(missing_id).unwrap_err();
    assert_eq!(err, QualError::RecordNotFound { id: missing_id.to_string() });
    assert_eq!(err.code(), ERR_RECORD_NOT_FOUND);

    let out = invoke_cli(&journal, "1.4.0", &mut activity_log, &["show-record", "--id", missing_id, "--json"]);
    assert_eq!(out.envelope.outcome, Outcome::Failed);
    assert_eq!(out.envelope.journal_entry_hash, None);
    let error_body = out.envelope.error.as_ref().expect("error body");
    assert_eq!(error_body.code, ErrorCode::NotFound);
    assert!(error_body.message.contains(ERR_RECORD_NOT_FOUND));
    let CValue::Obj(details) = error_body.details.as_ref().expect("details") else {
        panic!("expected details object");
    };
    assert_eq!(details.get("qual_error"), Some(&CValue::Str(ERR_RECORD_NOT_FOUND.to_string())));

    // Record Journal is completely untouched (R0 never writes to Record Journal).
    assert_eq!(journal, snapshot_before);
    // Activity Log captured the R0 call (D-031).
    assert_eq!(activity_log.len(), 1);
    let logged = ActivityEntry::from_json(&activity_log[0].to_json().unwrap()).unwrap();
    assert_eq!(logged, activity_log[0]);

    // Confirm CORE.JRN.STREAM.ALOG stream routing also never sends R0 to StreamId::Record.
    let rec_ctx = RouteContext { recording: true, program_step: None, access_ledger: false };
    assert_eq!(route(SideEffect::R0, Locality::Local, &rec_ctx), Some(StreamId::Activity));
    let idle_ctx = RouteContext::default();
    assert_eq!(route(SideEffect::R0, Locality::Local, &idle_ctx), None);
}

#[test]
fn ac5_mcp_validate_show_record_equals_cli_json_byte_for_byte() {
    let journal = load_v14_fixture();
    let mut cli_log = Vec::new();
    let mut mcp_log = Vec::new();

    // 1. Status mode under 1.4.0 (QUALIFIED)
    let cli_14 = invoke_cli(&journal, "1.4.0", &mut cli_log, &["show-record", "--json"]);
    let mcp_14 = invoke_mcp(&journal, "1.4.0", &mut mcp_log, MCP_SHOW_RECORD, br"{}");
    assert_eq!(cli_14.stdout, mcp_14.encode().unwrap());

    // 2. Status mode under 1.5.0 (UNQUALIFIED_NEW_VERSION)
    let cli_15 = invoke_cli(&journal, "1.5.0", &mut cli_log, &["show-record", "--json"]);
    let mcp_15 = invoke_mcp(&journal, "1.5.0", &mut mcp_log, MCP_SHOW_RECORD, br"{}");
    assert_eq!(cli_15.stdout, mcp_15.encode().unwrap());

    // 3. Record-by-ID mode
    let status = QualificationStatus::from_value(cli_14.envelope.result.as_ref().unwrap()).unwrap();
    let oq_id = status.oq_record.unwrap();
    let cli_id = invoke_cli(&journal, "1.4.0", &mut cli_log, &["show-record", "--id", &oq_id, "--json"]);
    let mcp_params = format!(r#"{{"id":"{oq_id}"}}"#);
    let mcp_id = invoke_mcp(&journal, "1.4.0", &mut mcp_log, MCP_SHOW_RECORD, mcp_params.as_bytes());
    assert_eq!(cli_id.stdout, mcp_id.encode().unwrap());

    // 4. Error mode (validate.record_not_found)
    let bad_id = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let cli_err = invoke_cli(&journal, "1.4.0", &mut cli_log, &["show-record", "--id", bad_id, "--json"]);
    let mcp_err_params = format!(r#"{{"id":"{bad_id}"}}"#);
    let mcp_err = invoke_mcp(&journal, "1.4.0", &mut mcp_log, MCP_SHOW_RECORD, mcp_err_params.as_bytes());
    assert_eq!(cli_err.stdout, mcp_err.encode().unwrap());
    assert_eq!(cli_log.len(), 4);
    assert_eq!(mcp_log.len(), 4);
    assert_eq!(mcp_log[0].surface, Surface::Mcp);
}

#[test]
fn status_transitions_and_upgrade_lifecycle() {
    let mut journal = RecordJournal::new();

    // Empty journal -> NOT_QUALIFIED.
    let s0 = journal.status("1.4.0");
    assert_eq!(s0.status, StatusCode::NotQualified);
    assert_eq!(s0.iq_record, None);
    assert_eq!(s0.oq_record, None);

    // IQ PASS only -> NOT_QUALIFIED with iq_record set.
    let iq1 = journal.commit_record(sample_iq("1.4.0", Verdict::Pass)).unwrap();
    let s1 = journal.status("1.4.0");
    assert_eq!(s1.status, StatusCode::NotQualified);
    assert_eq!(s1.iq_record.as_deref(), Some(iq1.id.as_str()));
    assert_eq!(s1.oq_record, None);

    // OQ FAIL -> NOT_QUALIFIED with both IDs.
    let oq1_fail = journal.commit_record(sample_oq("1.4.0", Verdict::Fail)).unwrap();
    let s2 = journal.status("1.4.0");
    assert_eq!(s2.status, StatusCode::NotQualified);
    assert_eq!(s2.iq_record.as_deref(), Some(iq1.id.as_str()));
    assert_eq!(s2.oq_record.as_deref(), Some(oq1_fail.id.as_str()));

    // OQ PASS -> QUALIFIED using the latest OQ record.
    let oq2_pass = journal.commit_record(sample_oq("1.4.0", Verdict::Pass)).unwrap();
    let s3 = journal.status("1.4.0");
    assert_eq!(s3.status, StatusCode::Qualified);
    assert_eq!(s3.iq_record.as_deref(), Some(iq1.id.as_str()));
    assert_eq!(s3.oq_record.as_deref(), Some(oq2_pass.id.as_str()));

    // Upgrade to 1.5.0 before running IQ/OQ -> UNQUALIFIED_NEW_VERSION (REQ-005).
    let s_up = journal.status("1.5.0");
    assert_eq!(s_up.status, StatusCode::UnqualifiedNewVersion);
    assert_eq!(s_up.iq_record, None);
    assert_eq!(s_up.oq_record, None);

    // 1.5.0 IQ PASS -> still NOT_QUALIFIED until OQ passes.
    let iq15 = journal.commit_record(sample_iq("1.5.0", Verdict::Pass)).unwrap();
    let s_15_iq = journal.status("1.5.0");
    assert_eq!(s_15_iq.status, StatusCode::NotQualified);
    assert_eq!(s_15_iq.iq_record.as_deref(), Some(iq15.id.as_str()));
    assert_eq!(s_15_iq.oq_record, None);

    // 1.5.0 OQ PASS -> QUALIFIED with 1.5.0 IDs; 1.4.0 still QUALIFIED with 1.4.0 IDs.
    let oq15 = journal.commit_record(sample_oq("1.5.0", Verdict::Pass)).unwrap();
    let s_15_ok = journal.status("1.5.0");
    assert_eq!(s_15_ok.status, StatusCode::Qualified);
    assert_eq!(s_15_ok.iq_record.as_deref(), Some(iq15.id.as_str()));
    assert_eq!(s_15_ok.oq_record.as_deref(), Some(oq15.id.as_str()));

    let s_14_still = journal.status("1.4.0");
    assert_eq!(s_14_still.status, StatusCode::Qualified);
    assert_eq!(s_14_still.iq_record.as_deref(), Some(iq1.id.as_str()));
    assert_eq!(s_14_still.oq_record.as_deref(), Some(oq2_pass.id.as_str()));

    // Subsequent IQ FAIL invalidates qualification (latest wins).
    let iq15_fail = journal.commit_record(sample_iq("1.5.0", Verdict::Fail)).unwrap();
    let s_15_regressed = journal.status("1.5.0");
    assert_eq!(s_15_regressed.status, StatusCode::NotQualified);
    assert_eq!(s_15_regressed.iq_record.as_deref(), Some(iq15_fail.id.as_str()));
    assert_eq!(s_15_regressed.oq_record.as_deref(), Some(oq15.id.as_str()));
}

#[test]
fn core_journal_entry_roundtrip_and_tamper_detection() {
    let iq = sample_iq("1.4.0", Verdict::Pass);
    let oq = sample_oq("1.4.0", Verdict::Pass);
    let meta = sample_meta("1.4.0");
    let mut salts = sample_salts();

    let e1 = build_journal_entry(&iq, &meta, FIRST_SEQ, GENESIS_PREV, &mut salts).expect("build IQ entry");
    let e2 = build_journal_entry(&oq, &meta, FIRST_SEQ + 1, e1.header.hash, &mut salts).expect("build OQ entry");

    let q1 = extract_from_entry(&e1).expect("extract IQ").expect("is qual event");
    assert_eq!(q1.command_id, RUN_IQ_ID);
    assert_eq!(q1.id, e1.header.hash.to_string());
    assert_eq!(q1.result_digest, iq.result_digest().unwrap());
    assert_eq!(q1.record, iq);

    let q2 = extract_from_entry(&e2).expect("extract OQ").expect("is qual event");
    assert_eq!(q2.command_id, RUN_OQ_ID);
    assert_eq!(q2.id, e2.header.hash.to_string());
    assert_eq!(q2.result_digest, oq.result_digest().unwrap());
    assert_eq!(q2.record, oq);

    let journal = RecordJournal::from_entries(&[e1.clone(), e2]).expect("journal from CORE.JRN.ENTRY");
    let status = journal.status("1.4.0");
    assert_eq!(status.status, StatusCode::Qualified);
    assert_eq!(status.iq_record.as_deref(), Some(q1.id.as_str()));
    assert_eq!(status.oq_record.as_deref(), Some(q2.id.as_str()));

    // Fail closed if the payload is tampered after hashing.
    let mut tampered = e1;
    tampered.payload.values.insert(Field::Events, CValue::Arr(vec![sample_iq("1.4.0", Verdict::Fail).to_value()]));
    let err = extract_from_entry(&tampered).unwrap_err();
    assert_eq!(err.code(), "validate.invalid_journal");
}

#[test]
fn fail_closed_validation_refuses_oq_fields_on_iq_and_digest_mismatch() {
    let mut bad_iq = sample_iq("1.4.0", Verdict::Pass);
    bad_iq.rdl_subset_digest = Some(synthetic_digest("forbidden-on-iq"));
    assert_eq!(bad_iq.validate().unwrap_err().code(), "validate.invalid_record");

    // Tampered result_digest in .fid JSON is refused.
    let journal = load_v14_fixture();
    let json = String::from_utf8(journal.to_json().unwrap()).unwrap();
    let good_digest = journal
        .events()
        .iter()
        .find_map(|e| match e {
            fid_qual::JournalEvent::Qualification(q) => Some(q.result_digest.as_str().to_string()),
            _ => None,
        })
        .unwrap();
    let fake_digest = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let tampered = json.replacen(&good_digest, fake_digest, 1);
    assert_eq!(RecordJournal::from_json(tampered.as_bytes()).unwrap_err().code(), "validate.invalid_journal");
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

#[test]
fn registry_command_def_and_bus_schema_validation() {
    let defs = command_defs().expect("command_defs");
    assert_eq!(defs.len(), 1);
    let def = &defs[0];
    assert_eq!(def.id, "validate.show_record");
    assert_eq!(def.major, 1);
    assert_eq!(def.side_effect, SideEffect::R0);
    assert_eq!(def.determinism, fid_types::Determinism::D0);
    assert_eq!(def.mcp_projection.as_str(), "exposed");
    assert!(!def.human_only);

    // Passes CORE.REG.VAL declaration checks and manifest export cleanly.
    let cat = Catalogue::geo_frame().expect("geo_frame catalogue");
    let decls: Vec<CommandDecl> = defs.iter().map(CommandDecl::from_def).collect();
    let export = fid_registry::manifest::export_with(&decls, &cat).expect("export validate.show_record@1");
    assert_eq!(export.definitions.len(), 1);

    // Compiles and validates real parameters and results through fid-bus's JSON Schema engine.
    let params_compiled = Compiled::compile(&def.params_schema).expect("compile params_schema");
    let result_compiled = Compiled::compile(&def.result_schema).expect("compile result_schema");

    assert!(params_compiled.evaluate(&CValue::Obj(BTreeMap::new()), &NoHooks).is_ok());
    let mut with_id = BTreeMap::new();
    with_id.insert("id".to_string(), CValue::Str("sha256:abc".to_string()));
    assert!(params_compiled.evaluate(&CValue::Obj(with_id), &NoHooks).is_ok());

    let mut empty_id = BTreeMap::new();
    empty_id.insert("id".to_string(), CValue::Str(String::new()));
    assert!(params_compiled.evaluate(&CValue::Obj(empty_id), &NoHooks).is_err());

    let mut unknown_param = BTreeMap::new();
    unknown_param.insert("extra".to_string(), CValue::Bool(true));
    assert!(params_compiled.evaluate(&CValue::Obj(unknown_param), &NoHooks).is_err());

    // Validate both branches of `oneOf` in result_schema: QualificationRecord & QualificationStatus.
    assert!(result_compiled.evaluate(&sample_iq("1.4.0", Verdict::Pass).to_value(), &NoHooks).is_ok());
    assert!(result_compiled.evaluate(&sample_oq("1.4.0", Verdict::Fail).to_value(), &NoHooks).is_ok());
    let journal = load_v14_fixture();
    assert!(result_compiled.evaluate(&journal.status("1.4.0").to_value(), &NoHooks).is_ok());
    assert!(result_compiled.evaluate(&journal.status("1.5.0").to_value(), &NoHooks).is_ok());
    assert!(result_compiled.evaluate(&RecordJournal::new().status("1.4.0").to_value(), &NoHooks).is_ok());
}

#[test]
fn performance_budget_under_200ms_for_10000_events() {
    let mut journal = RecordJournal::new();
    for i in 0..9_990u32 {
        journal.push_other(format!("ev-{i:05}"), "measure.point@1".to_string()).unwrap();
    }
    for v in ["1.1.0", "1.2.0", "1.3.0", "1.4.0"] {
        journal.commit_record(sample_iq(v, Verdict::Pass)).unwrap();
        journal.commit_record(sample_oq(v, Verdict::Pass)).unwrap();
    }
    let last_iq = journal.commit_record(sample_iq("1.5.0", Verdict::Pass)).unwrap();
    let last_oq = journal.commit_record(sample_oq("1.5.0", Verdict::Pass)).unwrap();
    assert_eq!(journal.len(), 10_000);

    let start = Instant::now();
    let status = journal.status("1.5.0");
    let found = journal.get_record(&last_oq.id).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(status.status, StatusCode::Qualified);
    assert_eq!(status.iq_record.as_deref(), Some(last_iq.id.as_str()));
    assert_eq!(status.oq_record.as_deref(), Some(last_oq.id.as_str()));
    assert_eq!(found.id, last_oq.id);
    assert!(elapsed.as_millis() < 200, "10,000-event scan took {} ms (budget < 200 ms)", elapsed.as_millis());
}
