//! Integration tests for spec node `TRUST.ESIG.MEANING` (AC-1..AC-4,
//! REQ-001..REQ-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::principal::{
    Channel, ChannelKind, Identity, OsUser, Principal, PrincipalId, PrincipalKind, SessionId, SessionKey, SessionTable,
    UpgradeGrant,
};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_esig::command_defs;
use fid_esig::meaning::{
    builtin_meanings, invoke_cli, invoke_mcp, EsigCtx, MeaningCatalogue, Origin, BUILTIN_CODES, LIST_MEANINGS,
};
use fid_profile::{instantiate, CanonicalProfile, ProfileName};
use fid_types::{Determinism, SideEffect};
use std::collections::BTreeMap;
use std::fs;

const PROFILE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/profile/");
const EXPECTED_FILE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/trust/esig/meaning/expected_list.json");

struct TestChannel {
    kind: ChannelKind,
    identity: Identity,
    session: SessionKey,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(self.identity.clone())
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        None
    }
}

fn make_principal(table: &mut SessionTable, kind: PrincipalKind, subject: &str) -> Principal {
    let ch = match kind {
        PrincipalKind::Human => ChannelKind::Gui,
        PrincipalKind::Agent => ChannelKind::Mcp,
        PrincipalKind::Script => ChannelKind::Cli,
        PrincipalKind::Service => ChannelKind::Service,
        PrincipalKind::Device => ChannelKind::Device,
        PrincipalKind::System => ChannelKind::Internal,
    };
    let mut identity = Identity::new(PrincipalId::new(subject).unwrap());
    identity.os_user = Some(OsUser::new(subject).unwrap());
    let session = table.open_session(SessionId::new(&format!("{}-{subject}", ch.as_str())).unwrap()).unwrap();
    table.derive_principal(&TestChannel { kind: ch, identity, session }).unwrap()
}

fn load_expected() -> BTreeMap<String, CValue> {
    let bytes = fs::read(EXPECTED_FILE).unwrap();
    match parse(&bytes).unwrap() {
        CValue::Obj(m) => m,
        other => panic!("expected object, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// REQ-005: Append-only built-in codes pin

#[test]
fn req005_builtin_codes_are_pinned_and_append_only() {
    assert_eq!(BUILTIN_CODES, ["AUTHORED", "REVIEWED", "APPROVED", "RELEASED"]);
    let builtins = builtin_meanings();
    let codes: Vec<&str> = builtins.iter().map(|m| m.code.as_str()).collect();
    assert_eq!(codes, vec!["AUTHORED", "REVIEWED", "APPROVED", "RELEASED"]);
    for m in &builtins {
        assert_eq!(m.origin, Origin::Builtin);
        assert!(!m.label.is_empty());
        assert!(!m.description.is_empty());
    }
}

// ---------------------------------------------------------------------------
// AC-1: Built-in Lean profile -> `fid sign list-meanings --json` returns the
//       four built-in codes in fixed order

#[test]
fn ac1_lean_profile_lists_four_builtins_in_order() {
    let lean = CanonicalProfile::builtin(ProfileName::Lean).unwrap();
    let cat = MeaningCatalogue::from_profile(lean.document()).unwrap();
    assert_eq!(cat.list().len(), 4);

    // Also verify loading directly from the committed Lean canonical JSON fixture.
    let lean_bytes = fs::read(format!("{PROFILE_DIR}builtin/lean.json")).unwrap();
    assert_eq!(MeaningCatalogue::from_profile_json(&lean_bytes).unwrap(), cat);

    let mut table = SessionTable::new();
    let human = make_principal(&mut table, PrincipalKind::Human, "alice");
    let ctx = EsigCtx { principal: &human, catalogue: &cat };

    let cli_bytes = invoke_cli(&ctx, &["sign", "list-meanings", "--json"]).unwrap();
    let expected = load_expected();
    let want = to_canonical_bytes(expected.get("ac1_lean").unwrap()).unwrap();
    assert_eq!(cli_bytes, want);
}

// ---------------------------------------------------------------------------
// AC-2: `fixtures/profile/regulated-with-xqa.json` defining `X_QA_RELEASE` ->
//       5 entries with tenant code last and origin `profile`

#[test]
fn ac2_regulated_with_xqa_lists_five_entries_with_tenant_last() {
    let tenant_bytes = fs::read(format!("{PROFILE_DIR}regulated-with-xqa.json")).unwrap();
    let instantiated = instantiate(ProfileName::Regulated, Some(&tenant_bytes)).unwrap();
    let cat_from_doc = MeaningCatalogue::from_profile(instantiated.document()).unwrap();
    let cat_from_raw = MeaningCatalogue::from_profile_json(&tenant_bytes).unwrap();
    assert_eq!(cat_from_doc, cat_from_raw);

    let list = cat_from_doc.list();
    assert_eq!(list.len(), 5);
    assert_eq!(list[4].code.as_str(), "X_QA_RELEASE");
    assert_eq!(list[4].origin, Origin::Profile);

    let mut table = SessionTable::new();
    let human = make_principal(&mut table, PrincipalKind::Human, "alice");
    let ctx = EsigCtx { principal: &human, catalogue: &cat_from_doc };

    let cli_bytes = invoke_cli(&ctx, &["sign", "list-meanings", "--json"]).unwrap();
    let expected = load_expected();
    let want = to_canonical_bytes(expected.get("ac2_regulated_with_xqa").unwrap()).unwrap();
    assert_eq!(cli_bytes, want);

    // Multiple tenant codes sort ascending after built-ins.
    let multi = br#"{
      "tenant_meanings": [
        { "code": "X_ZEBRA", "label": "Zebra", "description": "Zebra sign-off.", "labels": { "de": "Zebra" } },
        { "code": "X_ALPHA", "label": "Alpha", "description": "Alpha sign-off." }
      ]
    }"#;
    let multi_cat = MeaningCatalogue::from_profile_json(multi).unwrap();
    let codes: Vec<&str> = multi_cat.list().iter().map(|m| m.code.as_str()).collect();
    assert_eq!(codes, vec!["AUTHORED", "REVIEWED", "APPROVED", "RELEASED", "X_ALPHA", "X_ZEBRA"]);
    assert_eq!(multi_cat.get("X_ZEBRA").unwrap().labels.get("de").map(String::as_str), Some("Zebra"));
}

// ---------------------------------------------------------------------------
// AC-3: Error paths (`bad-meaning-collision.json`, `bad-meaning-syntax.json`,
//       duplicate code, missing `X_` prefix) -> `meaning.invalid` naming code

#[test]
fn ac3_invalid_tenant_meanings_fail_with_meaning_invalid() {
    // 1. Collision with built-in `APPROVED`.
    let collision_bytes = fs::read(format!("{PROFILE_DIR}bad-meaning-collision.json")).unwrap();
    let err_col = MeaningCatalogue::from_profile_json(&collision_bytes).unwrap_err();
    assert_eq!(err_col.code(), "meaning.invalid");
    assert_eq!(err_col.offending_code(), Some("APPROVED"));
    assert!(err_col.to_string().contains("APPROVED"));

    // 2. Bad syntax `x_lower`.
    let syntax_bytes = fs::read(format!("{PROFILE_DIR}bad-meaning-syntax.json")).unwrap();
    let err_syn = MeaningCatalogue::from_profile_json(&syntax_bytes).unwrap_err();
    assert_eq!(err_syn.code(), "meaning.invalid");
    assert_eq!(err_syn.offending_code(), Some("x_lower"));
    assert!(err_syn.to_string().contains("x_lower"));

    // 3. Duplicate tenant code.
    let dup = br#"{
      "tenant_meanings": [
        { "code": "X_DUP", "label": "One", "description": "First." },
        { "code": "X_DUP", "label": "Two", "description": "Second." }
      ]
    }"#;
    let err_dup = MeaningCatalogue::from_profile_json(dup).unwrap_err();
    assert_eq!(err_dup.code(), "meaning.invalid");
    assert_eq!(err_dup.offending_code(), Some("X_DUP"));

    // 4. Valid uppercase code lacking `X_` prefix.
    let no_prefix = br#"{
      "tenant_meanings": [
        { "code": "QA_RELEASE", "label": "QA", "description": "Missing X_ prefix." }
      ]
    }"#;
    let err_pfx = MeaningCatalogue::from_profile_json(no_prefix).unwrap_err();
    assert_eq!(err_pfx.code(), "meaning.invalid");
    assert_eq!(err_pfx.offending_code(), Some("QA_RELEASE"));
}

// ---------------------------------------------------------------------------
// AC-4: MCP `sign_list_meanings` equals CLI JSON + command declaration checks

#[test]
fn ac4_mcp_equals_cli_json_and_command_def_is_r0_d0() {
    let lean = CanonicalProfile::builtin(ProfileName::Lean).unwrap();
    let cat = MeaningCatalogue::from_profile(lean.document()).unwrap();

    let mut table = SessionTable::new();
    let human = make_principal(&mut table, PrincipalKind::Human, "alice");
    let agent = make_principal(&mut table, PrincipalKind::Agent, "agent-1");
    let ctx_human = EsigCtx { principal: &human, catalogue: &cat };
    let ctx_agent = EsigCtx { principal: &agent, catalogue: &cat };

    let cli_json = invoke_cli(&ctx_human, &["sign", "list-meanings", "--json"]).unwrap();
    let mcp_json = invoke_mcp(&ctx_agent, "sign_list_meanings", b"{}").unwrap();
    assert_eq!(mcp_json, cli_json);

    // Extra MCP parameter is refused.
    assert_eq!(invoke_mcp(&ctx_agent, "sign_list_meanings", br#"{"extra":1}"#).unwrap_err().code(), "bad_params");

    // Registry declaration is `sign.list_meanings@1`, R0, D0, Activity Log only.
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(format!("{}@{}", defs[0].id, defs[0].major), LIST_MEANINGS);
    assert_eq!(defs[0].side_effect, SideEffect::R0);
    assert_eq!(defs[0].determinism, Determinism::D0);
    assert!(defs[0].effects.is_empty());
}
